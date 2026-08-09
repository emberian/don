//! Extract one immutable, same-group retail checksum checkpoint from an RCX.

#[path = "../world_walk_checkpoint_frontier.rs"]
mod world_walk_checkpoint_frontier;

use don_replay::Replay;
use std::io::Write;
use std::path::PathBuf;
use world_walk_checkpoint_frontier::{checkpoint_json, extract_world_checkpoint};

fn main() {
    if let Err(error) = run() {
        eprintln!("don-world-checkpoint: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let replay_path = PathBuf::from(
        args.next()
            .ok_or("usage: don-world-checkpoint FILE.rcx GROUP OUTPUT.json")?,
    );
    let group = args
        .next()
        .ok_or("missing lockstep group")?
        .to_string_lossy()
        .parse::<i32>()
        .map_err(|error| format!("invalid lockstep group: {error}"))?;
    let output_path = PathBuf::from(args.next().ok_or("missing output path")?);
    if args.next().is_some() {
        return Err("unexpected extra argument".to_owned());
    }
    let replay = Replay::open(&replay_path).map_err(|error| error.to_string())?;
    let evidence = extract_world_checkpoint(&replay_path, &replay, group)
        .map_err(|error| error.to_string())?;
    let encoded = checkpoint_json(&evidence);
    let parent = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    if output_path.exists() {
        return Err("refusing to replace output".to_owned());
    }
    let temporary_path = parent.join(format!(
        ".{}.tmp-{}",
        output_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("world-checkpoint"),
        std::process::id(),
    ));
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)
        .map_err(|error| format!("refusing temporary-output collision: {error}"))?;
    let result = output
        .write_all(encoded.as_bytes())
        .and_then(|_| output.sync_all())
        .and_then(|_| output.metadata())
        .and_then(|metadata| {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&temporary_path, permissions)
        })
        .and_then(|_| std::fs::hard_link(&temporary_path, &output_path));
    drop(output);
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(format!("cannot publish checkpoint atomically: {error}"));
    }
    std::fs::remove_file(&temporary_path).map_err(|error| error.to_string())?;
    let published = std::fs::metadata(&output_path).map_err(|error| error.to_string())?;
    if published.len() != encoded.len() as u64 || !published.permissions().readonly() {
        return Err("published checkpoint identity/permissions drift".to_owned());
    }
    println!("{}", output_path.display());
    Ok(())
}
