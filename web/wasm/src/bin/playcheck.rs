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
    let active_players = args
        .get(base + 4)
        .filter(|value| !value.is_empty() && value.as_str() != "-")
        .map(|value| {
            value
                .split(',')
                .map(|slot| slot.parse::<u32>().expect("invalid roster slot"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for (index, slot) in active_players.iter().enumerate() {
        assert!(
            *slot < 4 && !active_players[..index].contains(slot),
            "digest roster must contain unique slots in 0..4"
        );
    }
    let game = game_create(seed as u32, (seed >> 32) as u32);
    assert!(!game.is_null(), "game_create failed");
    if digest_mode {
        // Browser `freshDigest` receives the same explicit roster argument. Never inherit
        // an unrelated live browser session or silently force the four-player match here.
        if let Some(&local_player) = active_players.first() {
            let mut mask = 0u32;
            let mut packed_teams = 0u32;
            for who in 0..4u32 {
                let active = active_players.contains(&who);
                if active {
                    mask |= 1 << who;
                }
                let team = if active { who } else { 8 };
                packed_teams |= team << (who * 8);
            }
            assert_eq!(
                unsafe { game_start_manual_teams(game, mask, packed_teams, 0, local_player, 0) },
                1,
                "manual PlayerSetup/team activation failed"
            );
        }
    }
    let initial_save = unsafe {
        if digest_mode {
            0
        } else {
            assert_eq!(game_save(game), 1, "initial authoritative save was refused");
            game_save_len(game)
        }
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
