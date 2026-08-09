# `don_env` — the RL environment

```sh
bash python/build.sh                                   # builds crates/don-env -> _don_env.so
PYTHONPATH=python python3 python/smoke_test.py --envs 256
```

`numpy` is the only hard dependency. `gymnasium` and `pettingzoo` are optional; the
wrappers degrade to duck-typed stand-ins with the same surface when they are absent.

## The shape

```python
from don_env import DonVecEnv
env = DonVecEnv(num_envs=256, num_agents=2, grid_w=64, grid_h=64)

ua, pa = env.sample_masked_actions_native()     # uniform under the mask, in Rust
obs, reward, done, trunc, info = env.step(ua, pa)
```

* `obs['spatial']`   `(E, A, 12, H, W)` f32 — feature planes
* `obs['entities']`  `(E, A, N, 16)` f32 — entity list, columns named after `Unit`/`Object` fields
* `obs['globals']`   `(E, A, 24)` f32 — `LeaderData` economy/score/outcome counters
* `obs['unit_mask']` `(E, A, C, 137)` uint8 — **bit-packed**, LSB first
* actions: `(E, A, C, 10)` int32 per entity, `(E, A, 5)` int32 per player

Every array aliases Rust memory and is invalidated by the next `step`. Copy what you keep.

## Action space

Ten independently masked heads per controlled entity — `Verb, TargetX, TargetY,
TargetEntity, Type, QueuePos, Stance, Form, OrderMods, Count` — plus five per player —
`Verb, TargetPlayer, Good, Amount, Treaty`. The 33 unit verbs and 16 player verbs are the
agent-emittable subset of the engine's own 82 `CommandTypes` opcodes; the other 33 are
classified `SELECTION` / `UI` / `ADMIN` / `CHEAT` and a Rust test asserts the partition is
exact.

Masks are packed because the `Type` head is 806 wide (the engine's whole `TypeIndex`
universe). Unpack per head with `env.unpack_unit_masks()`, or keep the packed form and
unpack on the GPU.

## Front ends

| module | class | shape |
|---|---|---|
| `don_env.core` | `DonVecEnv` | batched, multi-agent — the primitive |
| `don_env.gym_vector` | `DonGymVectorEnv` | Gymnasium `VectorEnv`; agent 0 is the policy, the rest come from an `opponent` callable (self-play / league hook) |
| `don_env.pettingzoo_env` | `DonVectorParallelEnv` | PettingZoo `ParallelEnv`, agent-keyed, still batched |
| `don_env.pettingzoo_env` | `DonParallelEnv` | PettingZoo `ParallelEnv`, one game |

## Reward

`env.reward_weights` is a linear combination over 20 named terms — the eleven `LeaderData`
score fields as per-step deltas, six outcome-counter deltas, and `win`/`loss`/`alive`.
Default is sparse (`win=+1, loss=-1`); shaping is opt-in via `set_reward_weights(**w)`.
The raw term vector is in `info['reward_terms']` every step, so arbitrary non-linear
shaping is still available in Python without a callback on the hot path.

## Before you read a training curve

```python
for kind, text in env.provenance():
    print(kind, text)
print(env.unimplemented())    # verbs the policy emitted that have no dynamics yet
print(env.apply_stats())      # illegal must be 0 if you sampled under the mask
```

The environment surface is derived. Most of the *dynamics* behind it are not yet. See
`docs/tracks/rl-env.md`.
