//! donfeed — least-data, read-only RoNtoy economy observation stream.
//!
//! The Windows process handle requests query/read rights only. The target is never
//! suspended, injected into, or written. Default cadence is intentionally 1 Hz.

#[cfg(not(windows))]
fn main() {
    eprintln!("donfeed is a Windows capture executable; cross-build it with cargo-xwin");
}

#[cfg(windows)]
mod windows {
    use donscan::live::{
        economy_snapshot, encode_economy_ndjson, hex_bytes, sha256, supported_source_identity,
        ObservationMeta, PREFERRED_IMAGE_BASE, SOURCE_SHA256,
    };
    use donscan::win;
    use std::fs::OpenOptions;
    use std::io::{self, BufWriter, Write};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const DEFAULT_PROCESS: &str = "riseofnations.exe";

    #[derive(Debug)]
    struct Args {
        pid: Option<u32>,
        process: String,
        base: Option<u32>,
        hz: u32,
        count: Option<u64>,
        out: Option<String>,
        append: bool,
        session_id: Option<u64>,
        max_errors: u32,
    }

    impl Default for Args {
        fn default() -> Self {
            Self {
                pid: None,
                process: DEFAULT_PROCESS.into(),
                base: None,
                hz: 1,
                count: None,
                out: None,
                append: false,
                session_id: None,
                max_errors: 3,
            }
        }
    }

    const USAGE: &str = "\
donfeed 0.1.0 - read-only RoNtoy economy observation stream

USAGE: donfeed [options]

  --pid <n>          target pid (default: first process matching --process)
  --process <name>   process name (default: riseofnations.exe)
  --base <hex>       override detected game image base
  --hz <1..15>       samples per second (default 1)
  --count <n>        stop after n observations (default: run until interrupted)
  --once             alias for --count 1
  --out <path>       write NDJSON to path (default stdout)
  --append           append rather than truncate --out
  --session <hex>    stable session id override
  --max-errors <n>   stop after n consecutive unavailable captures (default 3)
  -h, --help         this help
";

    fn parse_u32_hex(value: &str, what: &str) -> Result<u32, String> {
        let raw = value.trim_start_matches("0x").trim_start_matches("0X");
        u32::from_str_radix(raw, 16).map_err(|_| format!("bad {what} {value:?}"))
    }

    fn parse_u64_hex(value: &str, what: &str) -> Result<u64, String> {
        let raw = value.trim_start_matches("0x").trim_start_matches("0X");
        u64::from_str_radix(raw, 16).map_err(|_| format!("bad {what} {value:?}"))
    }

    fn parse_args() -> Result<Args, String> {
        let mut args = Args::default();
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0usize;
        while i < argv.len() {
            let key = argv[i].as_str();
            let next = |i: &mut usize| -> Result<&str, String> {
                *i += 1;
                argv.get(*i)
                    .map(String::as_str)
                    .ok_or_else(|| format!("{key} needs a value"))
            };
            match key {
                "-h" | "--help" => {
                    print!("{USAGE}");
                    std::process::exit(0);
                }
                "--pid" => {
                    let value = next(&mut i)?;
                    args.pid = Some(value.parse().map_err(|_| format!("bad --pid {value:?}"))?);
                }
                "--process" => args.process = next(&mut i)?.to_owned(),
                "--base" => args.base = Some(parse_u32_hex(next(&mut i)?, "--base")?),
                "--hz" => {
                    let value = next(&mut i)?;
                    args.hz = value.parse().map_err(|_| format!("bad --hz {value:?}"))?;
                    if !(1..=15).contains(&args.hz) {
                        return Err("--hz must be in 1..=15".into());
                    }
                }
                "--count" => {
                    let value = next(&mut i)?;
                    let count = value
                        .parse()
                        .map_err(|_| format!("bad --count {value:?}"))?;
                    if count == 0 {
                        return Err("--count must be positive".into());
                    }
                    args.count = Some(count);
                }
                "--once" => args.count = Some(1),
                "--out" => args.out = Some(next(&mut i)?.to_owned()),
                "--append" => args.append = true,
                "--session" => args.session_id = Some(parse_u64_hex(next(&mut i)?, "--session")?),
                "--max-errors" => {
                    let value = next(&mut i)?;
                    args.max_errors = value
                        .parse()
                        .map_err(|_| format!("bad --max-errors {value:?}"))?;
                    if args.max_errors == 0 {
                        return Err("--max-errors must be positive".into());
                    }
                }
                other => return Err(format!("unknown argument {other:?}\n\n{USAGE}")),
            }
            i += 1;
        }
        if args.append && args.out.is_none() {
            return Err("--append requires --out".into());
        }
        Ok(args)
    }

    fn generated_session_id(pid: u32) -> u64 {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        time.rotate_left(17) ^ (pid as u64) << 32 ^ std::process::id() as u64
    }

    fn output(args: &Args) -> Result<Box<dyn Write>, String> {
        if let Some(path) = &args.out {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(args.append)
                .truncate(!args.append)
                .open(path)
                .map_err(|e| format!("open {path:?}: {e}"))?;
            Ok(Box::new(BufWriter::new(file)))
        } else {
            Ok(Box::new(BufWriter::new(io::stdout())))
        }
    }

    pub fn run() -> Result<(), String> {
        let args = parse_args()?;
        let processes = win::list_processes();
        let pid = match args.pid {
            Some(pid) => pid,
            None => {
                let wanted = args.process.to_ascii_lowercase();
                processes
                    .iter()
                    .find(|(_, name)| name.to_ascii_lowercase() == wanted)
                    .map(|(pid, _)| *pid)
                    .ok_or_else(|| {
                        format!(
                            "no process named {:?}; use --pid or donscan --list",
                            args.process
                        )
                    })?
            }
        };
        let process = win::Proc::open(pid)
            .map_err(|code| format!("OpenProcess({pid}) failed, GetLastError={code}"))?;
        let module = win::find_game_image(&process).ok_or_else(|| {
            "supported Rise of Nations image not found; use donscan --modules".to_owned()
        })?;
        let detected_base = module.base as u32;
        if args.base.is_some_and(|base| base != detected_base) {
            return Err(format!(
                "--base does not match detected game image ({detected_base:#010x})"
            ));
        }
        let image_base = detected_base;
        let delta = (image_base as u64).wrapping_sub(PREFERRED_IMAGE_BASE as u64);
        let process_started_100ns = process
            .creation_time_100ns()
            .ok_or_else(|| "GetProcessTimes failed".to_owned())?;
        let image_path = process
            .image_path()
            .ok_or_else(|| "QueryFullProcessImageNameW failed".to_owned())?;
        let image_bytes = std::fs::read(&image_path)
            .map_err(|e| format!("read target image {image_path:?}: {e}"))?;
        let module_size = image_bytes.len() as u64;
        let module_sha256 = sha256(&image_bytes);
        if !supported_source_identity(module_size, &module_sha256) {
            return Err(format!(
                "unsupported target executable: size={module_size}, sha256={} (expected {})",
                hex_bytes(&module_sha256),
                hex_bytes(&SOURCE_SHA256)
            ));
        }
        let session_id = args.session_id.unwrap_or_else(|| generated_session_id(pid));
        let cadence = Duration::from_secs_f64(1.0 / args.hz as f64);
        let started = Instant::now();
        let mut writer = output(&args)?;
        eprintln!(
            "donfeed: read-only pid={pid} image_base={image_base:#010x} hz={} session={session_id:016x}",
            args.hz
        );

        let mut seq = 0u64;
        let mut consecutive_errors = 0u32;
        loop {
            let tick = Instant::now();
            let snapshot = economy_snapshot(&process, delta);
            let capture_us = tick.elapsed().as_micros().min(u32::MAX as u128) as u32;
            let line = encode_economy_ndjson(
                &snapshot,
                ObservationMeta {
                    session_id,
                    capture_seq: seq,
                    pid,
                    image_base,
                    process_started_100ns,
                    module_size,
                    module_sha256,
                    captured_unix_ms: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                        .min(u64::MAX as u128) as u64,
                    monotonic_us: started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                    capture_us,
                },
            );
            writer
                .write_all(line.as_bytes())
                .and_then(|_| writer.write_all(b"\n"))
                .and_then(|_| writer.flush())
                .map_err(|e| format!("write observation: {e}"))?;
            if !snapshot.ok {
                consecutive_errors += 1;
                eprintln!("donfeed: capture {seq} unavailable: {}", snapshot.note);
            } else {
                consecutive_errors = 0;
            }
            seq += 1;
            if consecutive_errors >= args.max_errors {
                return Err(format!(
                    "{} consecutive captures unavailable; target likely exited or changed",
                    consecutive_errors
                ));
            }
            if args.count.is_some_and(|count| seq >= count) {
                break;
            }
            std::thread::sleep(cadence.saturating_sub(tick.elapsed()));
        }
        Ok(())
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows::run() {
        eprintln!("donfeed: {error}");
        std::process::exit(1);
    }
}
