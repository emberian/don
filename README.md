# Descent of Nations: Thrones & Agents

An RL env and high-performance batch resimulation of Rise of Nations (2003).

---

Rise of Nations is a lockstep RTS — every machine runs the same simulation and they only
trade orders. Which means the engine checksums its own state every turn to catch drift,
and multiplayer replays *record those checksums*. So a reimplementation isn't a matter of
opinion: run it against a real recorded match and the game tells you which turn you got
wrong, and which subsystem.

Everything here is derived from the shipped binary and data files rather than from the
modding community's twenty years of notes. That started as discipline and turned out to
be necessary — a lot of what everyone knows is wrong:

- The damage formula isn't `attack × modifiers − armor`. Armor comes out **mid-chain**,
  with five more multipliers after it, and the minimum-1 floor is conditional.
- Borders aren't splines. That class just draws the ribbon; the sim is a per-cell integer
  radius, hard-capped at 44 tiles for a plain city and 96 fully teched.
- Squads don't lose soldiers to damage. Nothing in the damage path kills a guy.
- The AI cheats, by up to **2.29× income** at Toughest vs Tough, on identical towns.
- `economic.bhs`, shipped in 2003, has an infinite loop: it tests `== 0` where the engine
  returns `-1`.
- Four milliseconds matter — the tick is 67 ms, not the 15 Hz the data files imply.

## State

Reverse engineering is mostly done; the simulation is early.

- 22,750 functions and 19,914 types recovered, with field-level layouts
- `.rcx` replay command streams structurally decoded — 1,296,192 of 1,296,194 packages
  tile cleanly across 61 streams; payload interpretation remains partial
- Two processes play a lockstep game over TCP with a hand-built replacement netcode DLL
- RL env runs at ~98,000 steps/s across 1024 environments, ~4,600× real time
- …but 34% of actions it accepts still do nothing, because the mechanics behind them
  aren't written yet

Combat, economy, production, cities, borders, fog, projectiles, scoring, movement,
pathfinding, and terrain all have isolated ports with tests. Most are not yet wired into
the runnable tick; map generation and major command/order paths remain absent.

Live execution status: [`GOAL.md`](GOAL.md). Interrupted-work recovery ledger:
[`docs/RECOVERY.md`](docs/RECOVERY.md). Read-only live economy coaching:
[`docs/tracks/rontoy.md`](docs/tracks/rontoy.md). Replay browser:
[`docs/tracks/replay-viewer.md`](docs/tracks/replay-viewer.md).
