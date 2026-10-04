//! `GraphicEvents::process` 0x008E50A0 — step 16 of `Game::do_frame`.
//!
//! Despite the class name this is walked sim state: the body is a fog-reveal
//! sweep over `GraphicEvents::ambience_structs` (the walked
//! `GraphicEvents.ambience` array, `Array<AmbienceStruct>`, 23-byte rows).
//! Transcribed from `re/decomp-all/008e50a0.c`, cross-checked against the
//! Capstone listing of 0x008E50A0..0x008E521A (no `call` in the body, so it
//! consumes **zero** `game_random` draws).
//!
//! ```text
//! 008e50a3  eax = Game (*0x00c061ec)
//! 008e50ab  test byte [eax+0x550], 0x1f ; jne ret    -> only when Game::frame % 32 == 0
//! 008e50b8  cmp  [0x00c0b0c0], 0        ; jle ret    -> ambience_structs.length (GraphicEvents+0xb0)
//! 008e50cc  edx = [0x00c0b0cc]                       -> ambience_structs.list   (GraphicEvents+0xbc)
//! 008e50d4  edi = World (*0x00c06188)
//! loop (esi = i*0x18, i < length):
//! 008e50e0  old = row.ambience_seen                  (AmbienceStruct+0x16, u8)
//! 008e50ed  fy  = div_3_table[row.ambience_y >> 7]   (+0x8, sar 7; table[n] = n/3, FUN_00681db0)
//! 008e50f7  idx = fy * World::fog_xs                 (World+0xc)
//! 008e50fb  fx  = div_3_table[row.ambience_x >> 7]   (+0x4, sar 7)
//! 008e5108  idx += fx
//! 008e5111  row.ambience_seen |= World::seen[idx]    (World+0x15c, u8*)
//! 008e5132  if old != new:
//! 008e513a    diff = old ^ new                        (= bits newly set)
//! 008e5143    for p in 0..8, bit p of diff set:
//!               row.ambience_seen |= Leaders.list[p].ally_mask
//!               (0x00e40cb9 + p*0x6eec = Leader[p]+0x6929 `ally_mask`)
//! 008e5200  esi += 0x18
//! ```
//!
//! Step 16 runs before step 20 (`Game::frame++`), so the `& 0x1f` gate reads
//! the pre-increment frame. The function also never touches the per-slot
//! `GraphicEvent` roots, `entrench_*`, or `missile_offset_*`; those are
//! advanced elsewhere (object-side releases via `GraphicEvents::add`-family
//! callers, not this step).
//!
//! `World::seen` is `Save.world.seen` (walked at World+0x15c, `fog_size`
//! bytes), `Leader[p].ally_mask` is `Save.leaders.slots[p].body[0x6921]`
//! (`LeaderData::walk_data` 0x006d6750 walks +0x08..+0x692a as `body`, so
//! +0x6929 lands at body index 0x6921). A leader whose walked `flags & 1 == 0`
//! has no body on the stream; retail would still read its live `ally_mask`
//! byte (0 on a fresh/inactive slot), so we read 0.

use crate::tick::StepStatus;
use crate::Save;

pub const STATUS: StepStatus = StepStatus::Ported;

/// `Game+0x550` inside `Game::scalars` (see `tick::FRAME`).
const FRAME: usize = crate::tick::FRAME;
/// `AmbienceStruct` field offsets (PDB `AmbienceStruct`, sizeof 24).
const AMB_X: usize = 0x04; // ambience_x.value: int (WCoord)
const AMB_Y: usize = 0x08; // ambience_y.value: int (WCoord)
const AMB_SEEN: usize = 0x16; // ambience_seen: unsigned char
/// `World::fog_xs` (World+0xc) is `World.direct[1]` (`direct` starts at +0x8).
const WORLD_FOG_XS: usize = 1;
/// `Leader+0x6929 ally_mask` inside the walked `body` (+0x08..+0x692a).
const LEADER_ALLY_MASK: usize = 0x6929 - 0x08;

fn get_i32(buf: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

/// `div_3_table[n]` (FUN_00681db0: `table[n] = n / 3` for `0 <= n < 24*xs`).
/// The index is `WCoord >> 7` (arithmetic); retail indexes an `int*` with it,
/// so a negative coordinate would read before the table. Treat that as the
/// table's natural extension (`n / 3` with truncation) rather than panic —
/// it cannot occur for in-map ambience rows.
fn div_3(n: i32) -> i32 {
    n / 3
}

/// Fog cell index for an ambience row: `div_3(y >> 7) * fog_xs + div_3(x >> 7)`.
fn fog_index(x: i32, y: i32, fog_xs: i32) -> i64 {
    div_3(y >> 7) as i64 * fog_xs as i64 + div_3(x >> 7) as i64
}

pub fn run(save: &mut Save, effects: &mut Vec<String>) {
    // 008e50ab: test byte ptr [Game+0x550], 0x1f ; jne -> return
    let frame = get_i32(&save.game.scalars, FRAME);
    if frame & 0x1f != 0 {
        return;
    }
    // 008e50b8: cmp ambience_structs.length, 0 ; jle -> return
    if save.graphic_events.ambience.elems.is_empty() {
        return;
    }
    let fog_xs = save.world.direct.get(WORLD_FOG_XS).copied().unwrap_or(0);
    let seen = &save.world.seen;
    // Leader[p].ally_mask for p in 0..8 (0x00e40cb9 + p*0x6eec).
    let ally_mask: [u8; 8] = std::array::from_fn(|p| {
        save.leaders
            .slots
            .get(p)
            .and_then(|l| l.body.get(LEADER_ALLY_MASK))
            .copied()
            .unwrap_or(0)
    });

    let mut touched = 0usize;
    for (i, row) in save.graphic_events.ambience.elems.iter_mut().enumerate() {
        let d = &mut row.data;
        if d.len() < 23 {
            continue; // malformed row: leave untouched rather than guess
        }
        let old = d[AMB_SEEN];
        let idx = fog_index(get_i32(d, AMB_X), get_i32(d, AMB_Y), fog_xs);
        // 008e5111/008e511b: movzx al, byte [World::seen + idx]; or [row+0x16], al
        let Some(&fog) = usize::try_from(idx).ok().and_then(|i| seen.get(i)) else {
            // Retail would read out of the walked `seen` buffer; nothing
            // deterministic to transcribe — leave the row alone.
            continue;
        };
        let mut new = old | fog;
        if old != new {
            // 008e513a: diff = old ^ new (bits newly set by this sweep)
            let diff = old ^ new;
            for p in 0..8 {
                if diff & (1u8 << p) != 0 {
                    new |= ally_mask[p];
                }
            }
        }
        if new != old {
            d[AMB_SEEN] = new;
            touched += 1;
            effects.push(format!(
                "GraphicEvents.ambience[{i}].ambience_seen {old:#04x} -> {new:#04x} (fog idx {idx})"
            ));
        }
    }
    if touched == 0 {
        effects.push(format!(
            "GraphicEvents::process sweep at frame {frame}: {} ambience rows, none newly seen",
            save.graphic_events.ambience.elems.len()
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prim::Row;

    fn mk_save(frame: i32, fog_xs: i32, seen: Vec<u8>, rows: Vec<(i32, i32, u8)>, allies: [u8; 8]) -> Save {
        let mut s = Save::default();
        s.game.scalars = vec![0; 0x194];
        s.game.scalars[FRAME..FRAME + 4].copy_from_slice(&frame.to_le_bytes());
        s.world.direct = vec![0; 30];
        s.world.direct[WORLD_FOG_XS] = fog_xs;
        s.world.seen = seen;
        for p in 0..8 {
            let mut l = crate::sections::Leader::default();
            l.flags = 1;
            l.body = vec![0; 0x6922];
            l.body[LEADER_ALLY_MASK] = allies[p];
            s.leaders.slots.push(l);
        }
        for (x, y, seen) in rows {
            let mut r: Row<23> = Row::default();
            r.data[AMB_X..AMB_X + 4].copy_from_slice(&x.to_le_bytes());
            r.data[AMB_Y..AMB_Y + 4].copy_from_slice(&y.to_le_bytes());
            r.data[AMB_SEEN] = seen;
            s.graphic_events.ambience.elems.push(r);
        }
        s
    }

    fn seen_of(s: &Save) -> Vec<u8> {
        s.graphic_events.ambience.elems.iter().map(|r| r.data[AMB_SEEN]).collect()
    }

    #[test]
    fn div3_table_and_fog_index() {
        // FUN_00681db0: table[n] = n / 3; index is WCoord >> 7.
        assert_eq!(div_3(0), 0);
        assert_eq!(div_3(2), 0);
        assert_eq!(div_3(3), 1);
        assert_eq!(div_3(8), 2);
        // x = 3*128 -> (x>>7)=3 -> fx=1 ; y = 6*128 -> fy=2 ; idx = 2*fog_xs + 1
        assert_eq!(fog_index(3 * 128, 6 * 128, 10), 21);
    }

    #[test]
    fn gated_on_frame_mod_32() {
        // frame 33: test byte [frame],0x1f != 0 -> no write at all
        let mut s = mk_save(33, 1, vec![0xff], vec![(0, 0, 0)], [0; 8]);
        let mut fx = Vec::new();
        run(&mut s, &mut fx);
        assert_eq!(seen_of(&s), vec![0]);
        assert!(fx.is_empty());
        // frame 32 and frame 0 both pass the gate
        for f in [0, 32, 64, 0x120] {
            let mut s = mk_save(f, 1, vec![0x05], vec![(0, 0, 0)], [0; 8]);
            run(&mut s, &mut Vec::new());
            assert_eq!(seen_of(&s), vec![0x05], "frame {f}");
        }
    }

    #[test]
    fn ors_fog_byte_then_ally_masks_of_newly_set_bits() {
        // fog_xs = 4. Row at x=3*128 (fx=1), y=3*128 (fy=1) -> idx 5.
        let mut seen = vec![0u8; 16];
        seen[5] = 0b0000_0011; // players 0 and 1 have seen this cell
        // player 0 allied with 0 and 2 (mask 0b101); player 1 with 1 and 3 (0b1010);
        // player 2's mask must NOT be applied (bit 2 was not newly set by fog).
        let allies = [0b0000_0101, 0b0000_1010, 0b1111_0000, 0, 0, 0, 0, 0];
        let mut s = mk_save(32, 4, seen, vec![(3 * 128, 3 * 128, 0)], allies);
        let mut fx = Vec::new();
        run(&mut s, &mut fx);
        assert_eq!(seen_of(&s), vec![0b0000_1111]);
        assert_eq!(fx.len(), 1);
    }

    #[test]
    fn already_seen_bits_do_not_pull_allies() {
        // old already has bit 0; fog adds nothing new -> old != new is false,
        // ally masks are skipped even though bit 0 is set.
        let mut seen = vec![0u8; 4];
        seen[0] = 0b1;
        let allies = [0b1111_1111, 0, 0, 0, 0, 0, 0, 0];
        let mut s = mk_save(0, 2, seen, vec![(0, 0, 0b1)], allies);
        run(&mut s, &mut Vec::new());
        assert_eq!(seen_of(&s), vec![0b1]);
        // ...but a *different* newly set bit still pulls only its own mask.
        let mut seen = vec![0u8; 4];
        seen[0] = 0b11;
        let allies = [0b1000_0000, 0b0100_0000, 0, 0, 0, 0, 0, 0];
        let mut s = mk_save(0, 2, seen, vec![(0, 0, 0b1)], allies);
        run(&mut s, &mut Vec::new());
        assert_eq!(seen_of(&s), vec![0b0100_0011]);
    }

    #[test]
    fn per_row_independent_sweep() {
        // Two rows in different fog cells; only the second gains bits.
        let mut seen = vec![0u8; 9];
        seen[8] = 0b1000_0000; // (fx=2, fy=2) with fog_xs=3
        let mut s = mk_save(
            32,
            3,
            seen,
            vec![(0, 0, 0), (8 * 128, 8 * 128, 0b1)],
            [0, 0, 0, 0, 0, 0, 0, 0b0000_0001],
        );
        let mut fx = Vec::new();
        run(&mut s, &mut fx);
        assert_eq!(seen_of(&s), vec![0, 0b1000_0001]);
        assert_eq!(fx.len(), 1);
    }

    /// Oracle: for every consecutive live pair, ticking frame N must yield
    /// retail's N+1 `GraphicEvents.ambience` bytes exactly (both on sweep
    /// frames, N % 32 == 0, and on the gated-off frames in between).
    /// Skips when the proprietary captures are absent.
    #[test]
    fn live_pairs_reproduce_ambience_seen() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs");
        let Ok(rd) = std::fs::read_dir(&root) else {
            eprintln!("SKIP: live captures absent");
            return;
        };
        let mut dirs: Vec<_> = rd.flatten().map(|e| e.path()).filter(|d| d.join("manifest.json").is_file()).collect();
        dirs.sort();
        let mut pairs = 0usize;
        let mut sweeps = 0usize;
        for dir in dirs {
            let text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
            let mut frames: Vec<(i64, String)> = Vec::new();
            for seg in text.split("\"frame\":").skip(1) {
                let f = seg.trim_start().split(|c: char| !c.is_ascii_digit()).next().and_then(|t| t.parse().ok());
                let n = seg.split("\"save_name\":").nth(1).and_then(|s| s.split('"').nth(1)).unwrap_or_default();
                if let Some(f) = f {
                    frames.push((f, n.to_string()));
                }
            }
            for k in 0..frames.len().saturating_sub(1) {
                let ((fa, na), (fb, nb)) = (&frames[k], &frames[k + 1]);
                if fb - fa != 1 {
                    continue;
                }
                let raw_a = crate::container::load_svx(&dir.join(format!("{na}.svx"))).unwrap();
                let raw_b = crate::container::load_svx(&dir.join(format!("{nb}.svx"))).unwrap();
                let mut a = crate::load(&raw_a).unwrap().state;
                let b = crate::load(&raw_b).unwrap().state;
                let frame = get_i32(&a.game.scalars, FRAME);
                assert_eq!(frame as i64, *fa, "{} {na}: Game::frame", dir.display());
                let mut fx = Vec::new();
                run(&mut a, &mut fx);
                if frame & 0x1f == 0 {
                    sweeps += 1;
                    // Sweep executed: an effect record is emitted unless the
                    // array is empty (retail's `jle` early return).
                    assert_eq!(
                        fx.is_empty(),
                        a.graphic_events.ambience.elems.is_empty(),
                        "sweep frame {frame}: {} ambience rows, effects {fx:?}",
                        a.graphic_events.ambience.elems.len()
                    );
                    eprintln!(
                        "{} f{fa}: sweep over {} ambience rows",
                        dir.display(),
                        a.graphic_events.ambience.elems.len()
                    );
                } else {
                    assert!(fx.is_empty(), "gated frame {frame} wrote: {fx:?}");
                }
                let ours: Vec<&[u8]> = a.graphic_events.ambience.elems.iter().map(|r| r.data.as_slice()).collect();
                let theirs: Vec<&[u8]> = b.graphic_events.ambience.elems.iter().map(|r| r.data.as_slice()).collect();
                assert_eq!(ours, theirs, "{} f{fa}->f{fb} ambience rows", dir.display());
                pairs += 1;
            }
        }
        eprintln!("live pairs checked: {pairs} (sweep frames: {sweeps})");
        assert!(pairs > 0);
    }

    #[test]
    fn empty_ambience_is_a_noop() {
        let mut s = mk_save(0, 3, vec![0xff; 9], vec![], [0xff; 8]);
        let mut fx = Vec::new();
        run(&mut s, &mut fx);
        assert!(fx.is_empty());
    }
}
