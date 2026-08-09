# The /goal invocation

Copy the block below (everything after the `/goal `) to start standing-goal mode.

---

/goal Build Descent of Nations into one authoritative deterministic game core that simultaneously supports an independent, complete and enjoyable successor to Rise of Nations; a defensible retail-fidelity reference; and a high-performance RL/evaluation platform for strong non-cheating player AI and RoNEval. Keep this goal active until the full completion definition in /Users/ember/dev/don/GOAL.md is evidenced, not merely until a convenient subsystem turns green.

READ FIRST, EVERY SESSION: /Users/ember/dev/don/docs/CHARTER.md — it holds the methodology law, the fidelity tiers, the staged roadmap, the swarm doctrine, and the box-safety rules. /Users/ember/dev/don/docs/binary-ground-truth.md holds what we have already derived from the binary; do not re-derive it, and do not contradict it without new evidence. /Users/ember/dev/don/docs/prior-art-survey.md is CROSS-CHECK MATERIAL ONLY — never a source for an implemented value.

The one rule that matters most: ground truth is the binary and the shipped data files. Before writing any constant or formula, say out loud where it came from — a binary address, or a file and line in ron-data/. If the honest answer is "the wiki says so", STOP and go get it from the binary. Maintain the provenance ledger; a mechanic without a ledger entry is not done. Label every mechanic with its fidelity tier (A = SMT-proven over the full input domain; B = differentially tested against the real code, always stated with N and the input distribution; C = behaviorally faithful with measured divergence). Never inflate a tier, and never let differential testing call itself verification — we hold no formal semantics of Rust or x86, so nothing here is "verified" in the proof-assistant sense. Describe the work at that resolution in commits, in docs, and to me.

Treat /Users/ember/dev/don/GOAL.md as the live execution board. At the start of every cycle, read its current measured snapshot, the generated records it names, git status, and recent commits. Generated evidence overrides stale prose. Select the highest-value blockers across the whole north star; do not collapse the project into the first subsystem with an easy local test.

Current broad frontier:

- reconstruct a non-empty deterministic initial world from retail replays, drive decoded commands through the same authoritative command/order/system path used by the game, browser and RL environment, and reduce the first substantive checksum divergence;
- replace reachable MODEL substitutions and accepted-no-effect verbs with retail-sophisticated construction, gathering, combat, targeting, tactical, water, air, diplomacy, attrition, supply, victory and RNG consequences, or mask them honestly until they exist;
- keep Arena, RL, replay, browser, save/load, BHS/content and human play on that one core, with exact action masks and measured batch performance;
- grow strong observation-only AI through deterministic scripted/search/learned baselines and paired-seat evaluation, never privileged fog, free resources or implicit difficulty multipliers;
- finish the playable product flow, independently licensed presentation assets, BHS/mod compatibility and distributable tooling without letting polish conceal reachable drift;
- keep retail-control fail-closed and main-thread-only. Its v4 supervised scout move is live-proven, but /Users/ember/dev/don/schema/live/retail-control-stop-incident-v1.json leaves one explicit gate: re-exercise the hardened main-thread STOP acknowledgement in a fresh active match before calling the current lifecycle fully converged.

Swarm-cycle deliberately. Use modest parallel waves when work is genuinely independent. Give every lane absolute paths, concrete evidence addresses or generated records, an exclusive file set or explicit shared-API owner, the project-correct default, relevant tests, and a required return state: landed, research-only, partial, or no artifact. Include adversarial audit lanes that inspect actual code, bytes, artifacts and fidelity claims rather than trusting summaries. Reuse finished capacity on another product plane so breadth does not decay. Do not delegate ambiguous shared-file edits.

Land continuously on the permanent dev branch. Never create or switch branches, worktrees or stashes. Preserve unrelated user changes, stage named files rather than the whole tree, commit coherent tranches, run proportional focused checks plus the umbrella gates after shared API/state changes, push dev promptly, and leave a clean tree. A passing workspace test does not promote fidelity; a replay channel is meaningful only when it walks reconstructed non-empty state; product-readiness refusal remains a real blocker.

Names are product decisions, not implementation experiments. Use neutral role identifiers by default—`Ai` is sufficient for the generic observation-only policy—and do not introduce personas, codenames, brands or renamed public policy IDs without an explicit user decision and a demonstrated product need.

Work self-paced and choose reversible implementation details yourself. Stop only for a genuinely irreversible action, missing authority, unavailable external state, or a decision only the user can make. Bit-exactness and a strong player AI should pull the architecture, but never pull a fidelity claim beyond the evidence actually earned.
