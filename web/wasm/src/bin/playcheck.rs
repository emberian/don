//! Native twin of the playable WASM ABI.
//!
//! This intentionally drives `game_abi` rather than the retired standalone `GameWorld`,
//! so the native and browser checks exercise the same authoritative `don_sim::Sim`, the
//! same render projection, and the same deterministic save/load boundary.

use don_web::game_abi::*;

fn stage(bytes: &[u8], allocate: extern "C" fn(u32) -> *mut u8) {
    let ptr = allocate(bytes.len() as u32);
    assert!(
        !ptr.is_null() || bytes.is_empty(),
        "WASM ABI staging allocation failed"
    );
    if !bytes.is_empty() {
        // SAFETY: the ABI allocated exactly `bytes.len()` writable bytes and no other call
        // can resize that staging vector until this copy finishes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let digest_mode = args.get(1).map(String::as_str) == Some("digest");
    let base = if digest_mode { 2 } else { 1 };
    let gamedata = args
        .get(base)
        .and_then(|path| std::fs::read(path).ok())
        .unwrap_or_default();
    let playdata = args
        .get(base + 1)
        .and_then(|path| std::fs::read(path).ok())
        .unwrap_or_default();
    stage(&gamedata, game_gamedata_alloc);
    stage(&playdata, game_playdata_alloc);
    let seed = u64::from_str_radix(
        args.get(base + 2)
            .map(|value| value.trim_start_matches("0x"))
            .unwrap_or("c0ffee"),
        16,
    )
    .unwrap_or(0xC0FFEE);
    let frames = args
        .get(base + 3)
        .and_then(|value| value.parse().ok())
        .unwrap_or(600);
    let game = game_create(seed as u32, (seed >> 32) as u32);
    assert!(!game.is_null(), "game_create failed");
    let initial_save = unsafe {
        assert_eq!(game_save(game), 1, "initial authoritative save was refused");
        game_save_len(game)
    };
    unsafe { game_step(game, frames) };
    let (live, frame, rng, digest) = unsafe {
        let lo = game_digest_lo(game) as u64;
        let hi = game_digest_hi(game) as u64;
        (
            game_live(game),
            game_frame(game),
            game_rng_state(game),
            (hi << 32) | lo,
        )
    };
    println!(
        "seed 0x{seed:x} frames {frame} live {live} digest {digest:016x} rng {rng:08x} initial_save {initial_save}"
    );
    unsafe { game_destroy(game) };
}
