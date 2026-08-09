//! Fabricated environment for the two pure guy-turning primitives.
//!
//! `GuyData::turn_speed` (`0x005DE340`) is arithmetically self-contained but reaches its
//! owning `Unit` through `objects.lists[who][o]`, then reads the unit type and the two
//! turn-scale constants. `Guy::turn_angles` (`0x005D98C0`) is the side-effect-free form
//! of `Guy::turn_towards`: it writes the proposed angle through an output pointer instead
//! of calling `Guy::do_turn`. This fixture supplies exactly those reads. It deliberately
//! does not call `turn_towards`, whose `do_turn` tail reaches pivot and animation state.

use don_sim::systems::groups_guys::{
    GuyData, GuyEnv, UnitTypeStats, GUY_FLAG_FAST_FACE, GUY_WALK_LEN, GUY_WALK_LO,
    UNIT_MASK_TURN_SCALE2,
};

/// `Objects::lists[0]`; seven list pointers per owner, four bytes each.
pub const VA_OBJECT_LISTS: u32 = 0x00C0_AEC0;
/// Pointer to `Constants`; `turn_speed` reads dwords `+8` and `+0xC`.
pub const VA_CONSTANTS_PTR: u32 = 0x00C0_61E4;

pub const ARENA_BYTES: usize = 0x1000;
const GUY: usize = 0x000;
const LIST: usize = 0x100;
const UNIT: usize = 0x200;
const TYPE: usize = 0x400;
const CONSTANTS: usize = 0x800;

/// One flat, replayable input to both shipped Rust methods and the fabricated retail
/// environment. All integer widths match the actual fields read by the disassembly.
#[derive(Clone, Copy, Debug)]
pub struct Scenario {
    pub who: i8,
    pub object_index: i16,
    pub guy_num: i8,
    pub squad_size: i32,
    pub type_turn_speed: i32,
    pub turn_scale: i32,
    pub turn_scale2: i32,
    pub unit_mask_scale2: bool,
    pub track_dx: i32,
    pub track_dy: i32,
    pub last_speed: i32,
    pub avg_speed: i32,
    pub fast_face: bool,
    pub arg: i32,
    pub angle: u32,
    pub desired: u32,
    pub half: bool,
}

impl Default for Scenario {
    fn default() -> Self {
        Scenario {
            who: 0,
            object_index: 0,
            guy_num: 0,
            squad_size: 1,
            type_turn_speed: 0x2000_0000,
            turn_scale: 0x100,
            turn_scale2: 1,
            unit_mask_scale2: false,
            track_dx: 0,
            track_dy: 0,
            last_speed: 1,
            avg_speed: 0,
            fast_face: false,
            arg: 1,
            angle: 0,
            desired: 0x4000_0000,
            half: false,
        }
    }
}

impl Scenario {
    pub fn guy(self) -> GuyData {
        GuyData {
            angle: self.angle as i32,
            track_dx: self.track_dx,
            track_dy: self.track_dy,
            last_speed: self.last_speed,
            avg_speed: self.avg_speed,
            o: self.object_index,
            guy_flags: if self.fast_face {
                GUY_FLAG_FAST_FACE
            } else {
                0
            },
            who: self.who,
            guy_num: self.guy_num,
            ..GuyData::default()
        }
    }

    pub fn env(self) -> GuyEnv {
        GuyEnv {
            ut: UnitTypeStats {
                turn_speed: self.type_turn_speed,
                squad_size: self.squad_size,
                ..UnitTypeStats::default()
            },
            unit_mask_turn_scale2: self.unit_mask_scale2,
            turn_scale: self.turn_scale,
            turn_scale2: self.turn_scale2,
            ai_speed: 1,
            ..GuyEnv::default()
        }
    }

    /// The exact shipped Rust implementation under test.
    pub fn model_turn_speed(self) -> u32 {
        self.guy().turn_speed(&self.env(), self.arg)
    }

    /// The exact shipped Rust implementation under test. Retail's third argument is
    /// constrained to non-zero because the port's public method represents that live
    /// call shape and deliberately has no damped-argument parameter.
    pub fn model_turn_angles(self) -> (u32, u32) {
        self.guy().turn_angles(self.desired, &self.env(), self.half)
    }

    /// The shipped stateful implementation under test: return value plus the complete
    /// 155-byte synchronized `GuyData` range after turning.
    pub fn model_turn_towards(self) -> (u32, [u8; GUY_WALK_LEN]) {
        let mut guy = self.guy();
        let rem = guy.turn_towards(self.desired, &self.env());
        (rem, guy.walk_bytes())
    }

    /// Retail performs an unchecked unsigned divide. Signed `avg_speed/4 + 1` is zero
    /// exactly for `avg_speed in -7..=-4`; those inputs are reported as excluded instead
    /// of being allowed to kill the whole case child with `SIGFPE`.
    pub fn turn_speed_would_de(self) -> bool {
        let squad = (self.guy_num as i32) < self.squad_size;
        let crew_tracking_return = !squad && (self.track_dx != 0 || self.track_dy != 0);
        let fast_face_return = self.last_speed == 0 && self.fast_face;
        self.arg == 0 && !crew_tracking_return && !fast_face_return && self.avg_speed / 4 + 1 == 0
    }

    /// Deterministic, branch-biased draw. The mix deliberately spends far more mass on
    /// turn-rule and angle boundaries than a uniform word stream would.
    pub fn draw(w: [u64; 4]) -> Scenario {
        const TURN: [i32; 10] = [
            0,
            1,
            0xFF,
            0x100,
            0x0100_0000,
            0x0800_0000,
            0x2000_0000,
            0x4000_0000,
            i32::MAX,
            i32::MIN,
        ];
        const SCALE: [i32; 9] = [0, 1, 2, 100, 192, 255, 256, -1, i32::MAX];
        const AVG: [i32; 18] = [
            i32::MIN,
            -1024,
            -9,
            -8,
            -7,
            -6,
            -5,
            -4,
            -3,
            -1,
            0,
            1,
            3,
            4,
            7,
            8,
            1024,
            i32::MAX,
        ];
        const DELTA: [u32; 14] = [
            0,
            1,
            0x0222_221F,
            0x0222_2220,
            0x0222_2221,
            0x0FFF_FFFF,
            0x1000_0000,
            0x3FFF_FFFF,
            0x4000_0000,
            0x7FFF_FFFF,
            0x8000_0000,
            0x8000_0001,
            0xFFFF_FFFE,
            0xFFFF_FFFF,
        ];
        let angle = w[2] as u32;
        let delta = if w[2] & 3 == 0 {
            w[3] as u32
        } else {
            DELTA[(w[3] as usize) % DELTA.len()]
        };
        Scenario {
            who: ((w[0] >> 8) % 8) as i8,
            object_index: ((w[0] >> 12) % 4) as i16,
            guy_num: ((w[0] >> 16) % 128) as i8,
            squad_size: ((w[0] >> 24) % 129) as i32,
            type_turn_speed: TURN[(w[0] as usize) % TURN.len()],
            turn_scale: SCALE[((w[0] >> 32) as usize) % SCALE.len()],
            turn_scale2: SCALE[((w[0] >> 40) as usize) % SCALE.len()],
            unit_mask_scale2: w[1] & 1 != 0,
            track_dx: if w[1] & 2 != 0 { w[1] as i32 } else { 0 },
            track_dy: if w[1] & 4 != 0 {
                (w[1] >> 32) as i32
            } else {
                0
            },
            last_speed: if w[1] & 8 != 0 { 0 } else { w[1] as i32 },
            avg_speed: if w[1] & 16 != 0 {
                AVG[((w[1] >> 8) as usize) % AVG.len()]
            } else {
                (w[1] >> 32) as i32
            },
            fast_face: w[1] & 32 != 0,
            arg: if w[1] & 64 != 0 { 0 } else { w[2] as i32 | 1 },
            angle,
            desired: angle.wrapping_add(delta),
            half: w[1] & 128 != 0,
        }
    }
}

/// Hand-selected branch and arithmetic boundaries. This list is also a mutation-sensitivity
/// gate in the i686 test suite below.
pub fn edges() -> Vec<Scenario> {
    let b = Scenario::default();
    vec![
        b,
        Scenario {
            guy_num: 1,
            squad_size: 1,
            ..b
        }, // first crew guy
        Scenario {
            guy_num: 1,
            squad_size: 1,
            track_dx: 1,
            fast_face: true,
            last_speed: 0,
            ..b
        }, // crew tracking returns before fast-face
        Scenario {
            fast_face: true,
            last_speed: 0,
            ..b
        },
        Scenario {
            unit_mask_scale2: true,
            turn_scale2: 3,
            ..b
        },
        Scenario {
            arg: 0,
            avg_speed: 4,
            ..b
        },
        Scenario {
            arg: 0,
            avg_speed: -8,
            ..b
        },
        Scenario {
            arg: 0,
            type_turn_speed: 0x100,
            turn_scale: 2,
            avg_speed: i32::MAX,
            ..b
        }, // floor wins
        Scenario {
            arg: -1,
            type_turn_speed: i32::MIN,
            turn_scale: -1,
            ..b
        },
        Scenario {
            desired: 0x0222_221F,
            ..b
        },
        Scenario {
            desired: 0x0222_2220,
            ..b
        },
        Scenario {
            desired: 0x8000_0000,
            ..b
        },
        Scenario {
            desired: 0x8000_0001,
            ..b
        },
        Scenario {
            angle: 0xFFFF_FFF0,
            desired: 0x1000_0000,
            ..b
        },
        Scenario {
            desired: 0x2000_0001,
            ..b
        }, // one past the default step
        Scenario {
            desired: 0x2000_0001,
            half: true,
            ..b
        },
    ]
}

/// Write one scenario into a private scratch arena and install the two mapped-image
/// globals it needs. The caller owns and zeroes `arena` and guarantees a 32-bit process.
pub unsafe fn install(arena: *mut u8, s: Scenario, write_global: &impl Fn(u32, u32)) -> *mut u8 {
    let guy = arena.add(GUY);
    let list = arena.add(LIST);
    let unit = arena.add(UNIT);
    let ty = arena.add(TYPE);
    let constants = arena.add(CONSTANTS);

    let walk = s.guy().walk_bytes();
    std::ptr::copy_nonoverlapping(walk.as_ptr(), guy.add(GUY_WALK_LO), walk.len());
    std::ptr::write_unaligned(
        list.add(s.object_index as usize * 4) as *mut u32,
        unit as usize as u32,
    );
    std::ptr::write_unaligned(unit.add(0x18) as *mut u32, ty as usize as u32);
    std::ptr::write_unaligned(
        unit.add(0x68) as *mut u32,
        if s.unit_mask_scale2 {
            UNIT_MASK_TURN_SCALE2
        } else {
            0
        },
    );
    // `Guy::set_angle` / `Guy::do_turn` recurse from squad_size to total guy count when
    // this is the leader body. Equality means the fabricated unit has no extra crew: the
    // side-effecting leader path is exercised without inventing secondary Guy objects.
    std::ptr::write_unaligned(unit.add(0xE8) as *mut i32, s.squad_size.max(0));
    std::ptr::write_unaligned(ty.add(0x2C4) as *mut i32, s.type_turn_speed);
    std::ptr::write_unaligned(ty.add(0x304) as *mut i32, s.squad_size);
    std::ptr::write_unaligned(constants.add(8) as *mut i32, s.turn_scale);
    std::ptr::write_unaligned(constants.add(0xC) as *mut i32, s.turn_scale2);

    write_global(
        VA_OBJECT_LISTS + (s.who as u32 * 7 * 4),
        list as usize as u32,
    );
    write_global(VA_CONSTANTS_PTR, constants as usize as u32);
    guy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_corpus_kills_turn_speed_branch_deletions() {
        let e = edges();
        assert!(
            e.iter().any(|s| {
                let mut m = *s;
                m.fast_face = false;
                s.fast_face && s.model_turn_speed() != m.model_turn_speed()
            }),
            "fast-face deletion survived"
        );
        assert!(
            e.iter().any(|s| {
                let mut m = *s;
                m.unit_mask_scale2 = false;
                s.unit_mask_scale2 && s.model_turn_speed() != m.model_turn_speed()
            }),
            "second-scale deletion survived"
        );
        assert!(
            e.iter().any(|s| {
                let mut m = *s;
                m.arg = 1;
                s.arg == 0
                    && !s.turn_speed_would_de()
                    && s.model_turn_speed() != m.model_turn_speed()
            }),
            "damping deletion survived"
        );
        assert!(
            e.iter().any(|s| {
                let mut m = *s;
                m.track_dx = 0;
                m.track_dy = 0;
                (s.track_dx != 0 || s.track_dy != 0) && s.model_turn_speed() != m.model_turn_speed()
            }),
            "crew tracking early-return deletion survived"
        );
    }

    #[test]
    fn edge_corpus_kills_turn_angle_half_step_deletion() {
        assert!(edges().iter().any(|s| {
            let mut m = *s;
            m.half = false;
            s.half && s.model_turn_angles() != m.model_turn_angles()
        }));
    }

    #[test]
    fn edge_corpus_kills_do_turn_flag_side_effect_deletion() {
        assert!(
            edges().iter().any(|s| {
                let before = s.guy().walk_bytes();
                let (_, after) = s.model_turn_towards();
                // Absolute +0x9A in a walk that starts at +8.
                let flags = 0x9A - GUY_WALK_LO;
                let before_flags = u16::from_le_bytes([before[flags], before[flags + 1]]);
                let after_flags = u16::from_le_bytes([after[flags], after[flags + 1]]);
                before_flags & don_sim::systems::groups_guys::GUY_FLAG_NO_IDLE_TURN == 0
                    && after_flags & don_sim::systems::groups_guys::GUY_FLAG_NO_IDLE_TURN != 0
            }),
            "Guy::do_turn's `guy_flags |= 2` side effect is absent"
        );
    }

    #[test]
    fn de_domain_is_named_exactly() {
        for avg in -12..=4 {
            let s = Scenario {
                arg: 0,
                avg_speed: avg,
                ..Scenario::default()
            };
            assert_eq!(s.turn_speed_would_de(), (-7..=-4).contains(&avg));
        }

        let crew_tracking = Scenario {
            arg: 0,
            avg_speed: -4,
            guy_num: 1,
            squad_size: 1,
            track_dx: 1,
            ..Scenario::default()
        };
        assert!(!crew_tracking.turn_speed_would_de());
        let fast_face = Scenario {
            arg: 0,
            avg_speed: -4,
            last_speed: 0,
            fast_face: true,
            ..Scenario::default()
        };
        assert!(!fast_face.turn_speed_would_de());
    }
}
