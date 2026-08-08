//! The engine side of the production-AI contract.
//!
//! Everything in this module is derived from `riseofnations.exe`. Provenance is
//! recorded per item; addresses are static VAs at image base `0x00400000`.
//!
//! Reproduce with:
//! ```text
//! # AI cycle scheduler (Player::UpdateAI)
//! cd ron-bin && uv run --with capstone --with pefile python - <<'EOF'
//! ... disassemble 0x006B9620 .. 0x006B96FF ...
//! EOF
//! # production stage machine
//! cat re/decomp-all/006c1960.c
//! ```

/// Return codes the engine tests on the value the `ai` script returns.
///
/// [measured] `FUN_006C1960` case 1 compares the script result against `1`
/// and `3`; `economic.bhs` declares `labels { BLOCK_ON_THIS = 1,
/// DONT_BLOCK_ON_THIS, SCRIPT_DONE }`, i.e. 1/2/3. The two agree, which is a
/// cross-check that could have failed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ScriptResult {
    /// 1 — engine resets the AI stage counter to 0: the remaining compiled
    /// stages of this cycle are skipped.
    BlockOnThis = 1,
    /// 2 — engine advances to the next stage.
    DontBlockOnThis = 2,
    /// 3 — engine clears the "script has work" flag (`player+0x78C`) and
    /// advances to the next stage.
    ScriptDone = 3,
}

impl ScriptResult {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// The stage counter held at `player+0x788`.
///
/// [measured] `FUN_006C1960` is a 12-way switch on `player+0x788`
/// (`in_ECX[0x1e2]`). Stage 1 is the only one that runs the BHS production
/// script; stages 2..=11 are compiled C++ (call targets recorded below), and
/// the `default` arm resets the counter to 0.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AiStage {
    /// 0 — idle; the cycle has not been started this period.
    Idle,
    /// 1 — run the BHS production script named at `player+0x6EA4`.
    ProductionScript,
    /// 2 — `FUN_006C83E0` then `FUN_006C9DB0`.
    Stage2,
    /// 3 — `FUN_006C7A60`.
    Stage3,
    /// 4 — `FUN_006C6BA0`.
    Stage4,
    /// 5 — `FUN_006C6430`.
    Stage5,
    /// 6 — `FUN_006C40A0`.
    Stage6,
    /// 7 — `FUN_006C1BE0`.
    Stage7,
    /// 8 — `FUN_006C8AF0`; if it returns non-zero the cycle holds here.
    Stage8,
    /// 9 — `FUN_006C40A0` (same target as stage 6).
    Stage9,
    /// 10 — `FUN_006C1BE0` (same target as stage 7).
    Stage10,
    /// 11 — `FUN_006C8AF0`, then falls into the default arm (reset to 0).
    Stage11,
}

impl AiStage {
    pub fn from_i32(v: i32) -> Option<Self> {
        Some(match v {
            0 => AiStage::Idle,
            1 => AiStage::ProductionScript,
            2 => AiStage::Stage2,
            3 => AiStage::Stage3,
            4 => AiStage::Stage4,
            5 => AiStage::Stage5,
            6 => AiStage::Stage6,
            7 => AiStage::Stage7,
            8 => AiStage::Stage8,
            9 => AiStage::Stage9,
            10 => AiStage::Stage10,
            11 => AiStage::Stage11,
            _ => return None,
        })
    }

    /// True for the single stage that is authored in BHS rather than C++.
    pub fn is_scripted(self) -> bool {
        matches!(self, AiStage::ProductionScript)
    }
}

/// The four independently switchable AI subsystems.
///
/// [measured] bit positions in the second player flag word at `player+0x04`
/// (`DAT_00E3A394 + who0*0x6EEC`). `enable_*` clears the bit, `disable_*` sets
/// it, so a *set* bit means DISABLED.
///
/// | subsystem | bit | enable | disable |
/// |---|---|---|---|
/// | unit AI (all units) | `0x02` | `FUN_009FF8A0` | `FUN_009FF8E0` |
/// | production AI | `0x04` | `FUN_009FF5E0` | `FUN_009FF7A0` |
/// | combat AI | `0x08` | `FUN_009FF820` | `FUN_009FF860` |
/// | city AI | `0x10` | `FUN_009FFBA0` | `FUN_009FFBF0` |
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AiSubsystems(pub u32);

impl AiSubsystems {
    pub const UNIT_DISABLED: u32 = 0x02;
    pub const PRODUCTION_DISABLED: u32 = 0x04;
    pub const COMBAT_DISABLED: u32 = 0x08;
    pub const CITY_DISABLED: u32 = 0x10;

    pub fn production_enabled(self) -> bool {
        self.0 & Self::PRODUCTION_DISABLED == 0
    }
    pub fn combat_enabled(self) -> bool {
        self.0 & Self::COMBAT_DISABLED == 0
    }
    pub fn unit_enabled(self) -> bool {
        self.0 & Self::UNIT_DISABLED == 0
    }
    pub fn city_enabled(self) -> bool {
        self.0 & Self::CITY_DISABLED == 0
    }
}

/// Difficulty levels. Script-facing values are 1..=6; the byte stored in the
/// Game object at `Game+0x2B` is 0..=5.
///
/// [measured] `set_difficulty` @ `0x009E52F0` accepts `1..=6` and stores
/// `d - 1` into `byte [[0x00C061EC] + 0x2B]`; `get_difficulty` @ `0x009E5320`
/// returns that byte `+ 1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Difficulty {
    Easiest = 0,
    Easy = 1,
    Moderate = 2,
    Tough = 3,
    Tougher = 4,
    Toughest = 5,
}

impl Difficulty {
    pub fn from_stored(v: u8) -> Option<Self> {
        Some(match v {
            0 => Difficulty::Easiest,
            1 => Difficulty::Easy,
            2 => Difficulty::Moderate,
            3 => Difficulty::Tough,
            4 => Difficulty::Tougher,
            5 => Difficulty::Toughest,
            _ => return None,
        })
    }

    /// The signed percentage the engine adds to a non-human player's resource
    /// income.
    ///
    /// [measured, decompiled] `FUN_006D66A0` returns
    /// `-35 / -15 / -7 / 0 / +25 / +50` for effective difficulty `0..=5`, and
    /// `0` for the console (human) player. `FUN_006CE450`, the six-resource
    /// income accumulator, applies it as
    /// `income = ((pct + 100) * income) / 100` — **after** it has already
    /// stored the un-bonused rate into the per-resource display field, so the
    /// bonus does not appear in the AI's own reported gather rate.
    ///
    /// Tier C: read out of Ghidra's decompilation, integer-only path, not yet
    /// confirmed against a live process or the oracle.
    pub fn income_bonus_percent(self) -> i32 {
        match self {
            Difficulty::Easiest => -35,
            Difficulty::Easy => -15,
            Difficulty::Moderate => -7,
            Difficulty::Tough => 0,
            Difficulty::Tougher => 25,
            Difficulty::Toughest => 50,
        }
    }

    /// Apply the bonus exactly as `FUN_006CE450` does: a no-op when the
    /// percentage is zero, otherwise truncating integer arithmetic.
    pub fn apply_income_bonus(self, income: i32) -> i32 {
        let pct = self.income_bonus_percent();
        if pct == 0 {
            income
        } else {
            ((pct + 100) * income) / 100
        }
    }
}

/// Constants of the AI cycle scheduler in `Player::UpdateAI` @ `0x006B9620`.
///
/// [measured, disassembly] the prologue computes
///
/// ```text
///   period = 200 / *(int*)[0x00C061C0]        ; [0x00C061C0] is the game-speed cell
///   phase  = (player_index * 25 + Game[0x550]) % period
///   if (player->ai_stage != 0) { ProductionAiTick(player); return; }
///   if (Game[0x550] == 0 || phase == 0) -> start-of-cycle path @ 0x006B96F9
///   if (phase % 30 != 0) -> the rest of Player::Update
///   else -> age/tech check (0x006C9B90 / 0x006E0C80 / 0x006DB510 / 0x006C94F0)
/// ```
pub mod schedule {
    /// Numerator of the AI period, in sim ticks. `[measured]` immediate `0xC8`
    /// at `0x006B9629`.
    pub const PERIOD_NUMERATOR: i32 = 200;
    /// Per-player stagger, in sim ticks. `[measured]` immediate `0x19` at
    /// `0x006B9636` (`imul esi, [ebx+8], 0x19`, where `player+8` is the
    /// player index).
    pub const PLAYER_STAGGER: i32 = 25;
    /// Sub-period on which the age/tech check runs. `[measured]` the
    /// magic-number division by 30 at `0x006B967A..0x006B9698`.
    pub const TECH_CHECK_PERIOD: i32 = 30;
    /// `num_loops` handed to the production script. `[measured]` immediate
    /// `5` at `FUN_006C1960` case 1 (`*(undefined4 *)(iVar1 + 0x10) = 5`).
    pub const SCRIPT_NUM_LOOPS: i32 = 5;

    /// `period = PERIOD_NUMERATOR / game_speed`.
    pub fn period(game_speed: i32) -> i32 {
        PERIOD_NUMERATOR / game_speed
    }

    /// `phase = (player_index * 25 + tick) % period`.
    pub fn phase(player_index: i32, tick: i32, game_speed: i32) -> i32 {
        (player_index * PLAYER_STAGGER + tick) % period(game_speed)
    }

    /// True on the tick that starts a fresh AI cycle for this player.
    pub fn starts_cycle(player_index: i32, tick: i32, game_speed: i32) -> bool {
        tick == 0 || phase(player_index, tick, game_speed) == 0
    }
}

/// Player-record offsets touched by the production AI, in bytes from the base
/// of the player record (`0x00E3A390 + who0 * 0x6EEC`).
///
/// [measured] each is read off the decompilation / disassembly cited, and the
/// ones marked LIVE were additionally confirmed by reading the running game
/// (`riseofnations.exe` pid 148, image base `0x00D60000`) — see
/// `docs/tracks/ron-ai.md` §3.5. Nothing in this crate depends on them; they
/// are recorded so a heap crawler can use them.
pub mod player_offsets {
    /// Player array base and stride. `[measured]` every script host function
    /// indexes `&DAT_00E3A390 + (who - 1) * 0x6EEC`.
    pub const PLAYER_BASE: u32 = 0x00E3_A390;
    pub const PLAYER_STRIDE: u32 = 0x6EEC;
    pub const MAX_PLAYERS: u32 = 8;

    /// Flag word 0. bit0 = slot active, bit1 = in play, bit2 = human/console.
    /// `[measured]` `num_ai_players` @ `0x009E5EB0` counts active-and-not-bit2;
    /// `change_to_ai` @ `0x009E5D20` clears bit2. LIVE: human read
    /// `0x00080707` (bit 2 set), AI read `0x00080013` (bit 2 clear).
    pub const FLAGS0: u32 = 0x000;
    /// Flag word 1 — the AI subsystem disable bits (see [`super::AiSubsystems`]).
    /// LIVE: `0x00000000` for both players in a normal skirmish.
    pub const FLAGS1: u32 = 0x004;
    /// Player index (0-based). `[measured]` `in_ECX[2]` in `FUN_006C1960`.
    /// LIVE: read 1 for the second player.
    pub const INDEX: u32 = 0x008;
    /// Per-player difficulty override, `-1` when unset.
    /// `[measured]` `set_leader_difficulty` @ `0x009E5350` /
    /// `get_leader_difficulty` @ `0x009E53B0`.
    pub const LEADER_DIFFICULTY: u32 = 0x050;
    /// City count returned by `num_cities`. `[measured]` `FUN_009E92F0`.
    /// LIVE: read 2, matching the on-screen city count.
    pub const NUM_CITIES: u32 = 0x3F8;
    /// Length of the city-slot array scanned by `find_city_with_num`.
    /// `[measured]` `FUN_009EFF00`.
    pub const CITY_SLOTS: u32 = 0x408;
    /// AI stage counter. `[measured]` `in_ECX[0x1E2]` in `FUN_006C1960`.
    /// LIVE: 0 for both players at a tick whose phase was neither 0 nor a
    /// multiple of 30 — the case where the scheduler starts nothing.
    pub const AI_STAGE: u32 = 0x788;
    /// "Production script still has work" flag; cleared on `SCRIPT_DONE`.
    /// `[measured]` `in_ECX[0x1E3]`.
    pub const AI_SCRIPT_ACTIVE: u32 = 0x78C;
    /// Persistent `ref int step` handed to the production script.
    /// `[measured]` `in_ECX[0x1E4]`. LIVE: **35** for the AI player, inside
    /// `economic.bhs`'s 1..36 switch; 1 for the human, never advanced.
    pub const AI_SCRIPT_STEP: u32 = 0x790;
    /// Source of `boom_vs_rush` (the engine passes this value **plus 2**).
    /// `[measured]` `in_ECX[0x1B75]`. LIVE: `-1`, so the script saw 1 — and
    /// `economic.bhs` never reads the parameter.
    pub const AI_BOOM_VS_RUSH: u32 = 0x6DD4;
    /// `RString` naming the production script to run.
    /// `[measured]` `in_ECX[0x1BA9]`, passed as the script name to
    /// `ScriptRunByName` @ `0x0043D0E0`. LIVE: resolves to the eight UTF-16
    /// characters `economic` for the AI player, all zeros for the human.
    /// RString header layout observed live:
    /// `{ wchar_t* buf; u32 _; u16 len; u16 cap }`.
    pub const AI_SCRIPT_NAME: u32 = 0x6EA4;
    /// Pointer to the XOR-obfuscated player counter block.
    /// `[measured]` `(&DAT_00E41248)[who0 * 0x1BBB]`.
    pub const OBFUSCATED_BLOCK_PTR: u32 = 0x6EB8;
}

/// XOR masks the engine uses on obfuscated player counters.
///
/// [measured] each appears as a literal `xor` immediately after the load in the
/// cited host function. The engine stores `value ^ MASK`; readers XOR again.
/// `FUN_006CE450` writing `0x104BE` for the "Infinite resources" setting
/// decodes to `0x104BE ^ 0x8221 = 99999`, which is the cross-check that this
/// really is the stockpile field and that the mask is right.
pub mod obfuscation {
    /// Resource stockpile array, at `[player+0x6EB8] + res*4`.
    /// `[measured]` `num_type_with_queued` @ `0x009E9630`, `FUN_006CE450`.
    pub const RESOURCE_STOCKPILE: u32 = 0x8221;
    /// Player age, at `[player+0x6EB8] + 0xDC`. `[measured]` `age` @ `0x009E8F50`.
    pub const AGE: u32 = 0x62766;

    pub fn decode(stored: u32, mask: u32) -> u32 {
        stored ^ mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infinite_resources_constant_decodes_to_99999() {
        // The literal FUN_006CE450 stores when Game[0x2D] == 8 ("Infinite"
        // starting resources, index 8 of <CATEGORIES id="startingresources">).
        assert_eq!(obfuscation::decode(0x104BE, obfuscation::RESOURCE_STOCKPILE), 99_999);
    }

    #[test]
    fn difficulty_income_bonus_matches_fun_006d66a0() {
        assert_eq!(Difficulty::Easiest.income_bonus_percent(), -35);
        assert_eq!(Difficulty::Easy.income_bonus_percent(), -15);
        assert_eq!(Difficulty::Moderate.income_bonus_percent(), -7);
        assert_eq!(Difficulty::Tough.income_bonus_percent(), 0);
        assert_eq!(Difficulty::Tougher.income_bonus_percent(), 25);
        assert_eq!(Difficulty::Toughest.income_bonus_percent(), 50);
        // Truncating integer arithmetic, exactly as the binary does it.
        assert_eq!(Difficulty::Toughest.apply_income_bonus(101), 151);
        assert_eq!(Difficulty::Easiest.apply_income_bonus(101), 65);
        assert_eq!(Difficulty::Tough.apply_income_bonus(101), 101);
    }

    #[test]
    fn eight_players_are_staggered_across_the_period() {
        // 200 / 1 = 200, stagger 25 => player i starts its cycle at
        // tick == (200 - 25*i) mod 200.
        for idx in 0..8 {
            let start = (200 - 25 * idx) % 200;
            assert!(schedule::starts_cycle(idx, start, 1), "player {idx}");
            assert!(!schedule::starts_cycle(idx, start + 1, 1), "player {idx}");
        }
    }

    /// Live capture, `riseofnations.exe` pid 148, image base `0x00D60000`:
    /// tick 10353, game speed 1, AI player index 1, `player+0x788` (stage) 0.
    /// A stage of 0 is only consistent with the scheduler if that tick's phase
    /// is neither 0 (start of cycle) nor a multiple of 30 (tech check). If the
    /// period or the 25-tick stagger were wrong, this would not hold.
    #[test]
    fn live_capture_is_consistent_with_the_scheduler() {
        let (tick, idx, speed) = (10_353, 1, 1);
        let phase = schedule::phase(idx, tick, speed);
        assert_eq!(phase, 178);
        assert!(!schedule::starts_cycle(idx, tick, speed));
        assert_ne!(phase % schedule::TECH_CHECK_PERIOD, 0);
    }

    /// The live `RString` at `player+0x6EA4` carried `len = 8` and eight UTF-16
    /// characters spelling the script name.
    #[test]
    fn live_script_name_length_matches() {
        assert_eq!("economic".len(), 8);
    }

    #[test]
    fn only_one_of_twelve_stages_is_scripted() {
        let scripted = (0..12)
            .filter(|v| AiStage::from_i32(*v).unwrap().is_scripted())
            .count();
        assert_eq!(scripted, 1);
    }
}
