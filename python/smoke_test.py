#!/usr/bin/env python3
"""Smoke test and throughput measurement for the Descent of Nations RL environment.

Creates N envs, steps them with **masked** random actions, and reports steps per second.
It also checks the properties that make the numbers mean something:

* every sampled action is legal (``apply_stats()['illegal'] == 0``);
* observation buffers really alias Rust memory (a step changes them without a new array);
* the reward term vector and the engine score components are populated.

Run:  PYTHONPATH=python python3 python/smoke_test.py [--envs 64] [--steps 200]
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

from don_env import DonVecEnv  # noqa: E402
from don_env.spaces import HAVE_GYM, action_space, observation_space  # noqa: E402


def hr(title: str) -> None:
    print(f"\n\033[1m{title}\033[0m\n" + "-" * len(title))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--envs", type=int, default=64)
    ap.add_argument("--steps", type=int, default=200)
    ap.add_argument("--warmup", type=int, default=20)
    ap.add_argument("--agents", type=int, default=2)
    ap.add_argument("--grid", type=int, default=64)
    ap.add_argument("--max-controlled", type=int, default=32)
    ap.add_argument("--max-entities", type=int, default=64)
    ap.add_argument("--threads", type=int, default=0)
    ap.add_argument("--noop", action="store_true", help="measure the env with zero actions")
    ap.add_argument("--numpy-sampler", action="store_true",
                    help="sample under the mask in numpy instead of natively, to measure "
                         "the cost of unpacking the packed masks")
    args = ap.parse_args()

    env = DonVecEnv(
        num_envs=args.envs,
        num_agents=args.agents,
        grid_w=args.grid,
        grid_h=args.grid,
        max_entities=args.max_entities,
        max_controlled=args.max_controlled,
        start_units=16,
        max_steps=0,
        threads=args.threads,
        seed=12345,
    )
    rng = np.random.default_rng(0)

    hr("provenance — read this before any training curve")
    for kind, text in env.provenance():
        print(f"  {kind:14s} {text}")

    hr("spaces")
    spec = env.spec
    print(f"  gymnasium installed: {HAVE_GYM}")
    print(f"  unit action heads ({len(env.unit_heads)}):")
    for h in env.unit_heads:
        print(f"      {h.name:14s} size={h.size:<5d} mask_byte_offset={h.mask_offset}")
    print(f"  player action heads ({len(env.player_heads)}):")
    for h in env.player_heads:
        print(f"      {h.name:14s} size={h.size:<5d} mask_byte_offset={h.mask_offset}")
    print(f"  unit verbs   : {len(spec['unit_verbs'])} (from the 82 CommandTypes opcodes)")
    print(f"  player verbs : {len(spec['player_verbs'])}")
    print(f"  mask record  : {spec['unit_mask_record_bytes']} B/entity packed "
          f"(vs {sum(spec['unit_head_sizes'])} B unpacked bool)")
    print(f"  obs space    : {observation_space(spec)}")
    print(f"  act space    : {action_space(spec)}")

    hr("zero-copy check")
    obs = env.observation()
    ent = obs["entities"]
    print(f"  entities.flags.owndata = {ent.flags.owndata} (False == view into Rust memory)")
    base_ptr = ent.__array_interface__["data"][0]
    before = ent.copy()
    ua, pa = env.sample_masked_actions(rng)
    env.step(ua, pa)
    after_ptr = env.observation()["entities"].__array_interface__["data"][0]
    changed = not np.array_equal(before, env.observation()["entities"])
    print(f"  pointer stable across step: {base_ptr == after_ptr}")
    print(f"  contents changed in place : {changed}")

    hr("warmup + measurement")

    def draw():
        if args.noop:
            return noop_ua, noop_pa
        if args.numpy_sampler:
            return env.sample_masked_actions(rng)
        return env.sample_masked_actions_native()

    noop_ua, noop_pa = env.noop_actions()
    for _ in range(args.warmup):
        ua, pa = draw()
        env.step(ua, pa)

    sample_t = 0.0
    step_t = 0.0
    t_all = time.perf_counter()
    for _ in range(args.steps):
        t0 = time.perf_counter()
        ua, pa = draw()
        sample_t += time.perf_counter() - t0
        t0 = time.perf_counter()
        env.step(ua, pa)
        step_t += time.perf_counter() - t0
    wall = time.perf_counter() - t_all
    sampler = "noop" if args.noop else ("numpy" if args.numpy_sampler else "native")

    n, a = args.envs, args.agents
    frames_per_step = spec["frames_per_step"]
    tick_ms = spec["tick_ms"]
    env_steps = args.steps * n
    print(f"  envs={n} agents={a} grid={args.grid}x{args.grid} "
          f"max_controlled={args.max_controlled} steps={args.steps} sampler={sampler}")
    print(f"  wall {wall:.3f}s   env.step {step_t:.3f}s   masked sampling {sample_t:.3f}s")
    print()
    print(f"  env-steps/s (step only)     : {env_steps / step_t:,.0f}")
    print(f"  env-steps/s (whole loop)    : {env_steps / wall:,.0f}")
    print(f"  agent-steps/s (whole loop)  : {env_steps * a / wall:,.0f}")
    print(f"  sim frames/s (whole loop)   : {env_steps * frames_per_step / wall:,.0f}")
    rtf = env_steps * frames_per_step * (tick_ms / 1000.0) / wall
    print(f"  real-time factor            : {rtf:,.0f}x  "
          f"(a tick is {tick_ms} ms at Normal [measured])")

    hr("action outcomes")
    st = env.apply_stats()
    total = sum(st.values())
    for k, v in st.items():
        print(f"  {k:20s} {v:>12,}  {100.0 * v / max(total, 1):5.1f}%")
    ok = st["illegal"] == 0
    print(f"  masked sampling produced 0 illegal actions: {ok}")

    hr("verbs with no dynamics behind them")
    ui = env.unimplemented()
    if not ui:
        print("  none")
    for name, count in ui[:20]:
        print(f"  {name:28s} {count:>10,}")

    hr("reward + engine score")
    print(f"  weights (default is sparse outcome): "
          f"{ {k: v for k, v in env.reward_weights.items() if v} }")
    terms = env.buffers["reward_terms"]
    nz = [(t, float(terms[..., i].sum())) for i, t in enumerate(spec["reward_terms"])
          if abs(terms[..., i]).sum() > 0]
    print(f"  non-zero terms this step: {nz}")
    sc = env.scores()
    print(f"  scores array {sc.shape} (11 LeaderData fields + our unweighted total)")
    print(f"  env0 agent0: " + ", ".join(
        f"{n}={v}" for n, v in zip(list(spec["score_terms"]) + ["total(ours)"], sc[0, 0])))
    env.set_reward_weights(d_units_killed=0.05, d_score_economy=0.01)
    print(f"  after set_reward_weights: "
          f"{ {k: v for k, v in env.reward_weights.items() if v} }")

    hr("multi-agent front ends")
    from don_env.pettingzoo_env import DonParallelEnv, DonVectorParallelEnv
    from don_env.gym_vector import DonGymVectorEnv, random_masked_opponent

    pz = DonVectorParallelEnv(num_envs=4, num_agents=2, grid_w=32, grid_h=32, max_steps=0)
    o, _ = pz.reset()
    acts = {}
    ua, pa = pz.core.sample_masked_actions(rng)
    for i, ag in enumerate(pz.agents):
        acts[ag] = {"unit": ua[:, i], "player": pa[:, i]}
    o, r, term, trunc, _ = pz.step(acts)
    print(f"  PettingZoo parallel  agents={pz.agents} reward shapes="
          f"{ {k: tuple(v.shape) for k, v in r.items()} }")

    single = DonParallelEnv(num_agents=2, grid_w=32, grid_h=32, max_steps=0)
    o, _ = single.reset()
    ua, pa = single.core.sample_masked_actions(rng)
    o, r, term, trunc, _ = single.step(
        {ag: {"unit": ua[0, i], "player": pa[0, i]} for i, ag in enumerate(single.agents)}
    )
    print(f"  PettingZoo single    reward={r}")

    gv = DonGymVectorEnv(num_envs=4, num_agents=2, grid_w=32, grid_h=32, max_steps=0,
                         opponent=random_masked_opponent)
    o, _ = gv.reset(seed=1)
    ua, pa = gv.core.sample_masked_actions(rng)
    o, r, d, t, _ = gv.step({"unit": ua[:, 0], "player": pa[:, 0]})
    print(f"  Gymnasium vector     reward={r.shape} done={d.shape} "
          f"obs.spatial={o['spatial'].shape}")

    hr("verdict")
    checks = {
        "no illegal actions under the mask": ok,
        "observations alias Rust memory": not ent.flags.owndata,
        "observations change in place": changed,
        "engine score components populated": bool(sc.any()),
        "throughput measured": step_t > 0,
    }
    for k, v in checks.items():
        print(f"  [{'PASS' if v else 'FAIL'}] {k}")
    return 0 if all(checks.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
