"""Gymnasium `VectorEnv` semantics over `DonVecEnv`.

Single-agent view: agent 0 is the policy, agents 1.. are whatever `opponent_actions`
supplies (NOOP by default, a frozen policy for league play). That is the shape self-play
needs — the opponent slot is an argument, not a hard-coded scripted bot — so a league can
swap in a past checkpoint without touching the env.

Autoreset follows Gymnasium's `NextStep` mode: when an episode ends the returned
observation is already the first observation of the next one, and the final reward is in
the same step's `rewards`.
"""

from __future__ import annotations

from typing import Any, Callable

import numpy as np

from .core import DonVecEnv
from .spaces import action_space, observation_space

try:  # pragma: no cover
    from gymnasium.vector import VectorEnv as _GymVectorEnv

    _BASE = _GymVectorEnv
except ImportError:  # pragma: no cover
    class _BASE:  # type: ignore[no-redef]
        pass


OpponentPolicy = Callable[["DonGymVectorEnv"], tuple[np.ndarray, np.ndarray]]


def noop_opponent(env: "DonGymVectorEnv"):
    return env.core.noop_actions()


def random_masked_opponent(env: "DonGymVectorEnv"):
    return env.core.sample_masked_actions(env.rng)


class DonGymVectorEnv(_BASE):
    metadata = {"render_modes": [], "autoreset_mode": "NextStep"}

    def __init__(self, num_envs: int = 8, opponent: OpponentPolicy = noop_opponent, **kw: Any):
        self.core = DonVecEnv(num_envs=num_envs, **kw)
        self.num_envs = num_envs
        self.rng = np.random.default_rng(kw.get("seed", 0x5EED))
        self.opponent = opponent
        self.single_observation_space = observation_space(self.core.spec)
        self.single_action_space = action_space(self.core.spec)
        self.observation_space = self.single_observation_space
        self.action_space = self.single_action_space

    def _agent0(self, obs: dict[str, np.ndarray]) -> dict[str, np.ndarray]:
        return {k: v[:, 0] for k, v in obs.items()}

    def reset(self, *, seed: int | None = None, options: dict | None = None):
        if seed is not None:
            self.rng = np.random.default_rng(seed)
        obs = self.core.reset()
        return self._agent0(obs), {}

    def step(self, action: dict[str, np.ndarray]):
        """`action` is `{'unit': (E, C, 10), 'player': (E, 5)}` for agent 0."""
        ua, pa = self.opponent(self)
        ua = np.array(ua, dtype=np.int32, copy=True)
        pa = np.array(pa, dtype=np.int32, copy=True)
        ua[:, 0] = np.asarray(action["unit"], dtype=np.int32)
        pa[:, 0] = np.asarray(action["player"], dtype=np.int32)
        obs, rewards, dones, truncs, info = self.core.step(ua, pa)
        return self._agent0(obs), rewards[:, 0].copy(), dones.copy(), truncs.copy(), info

    def close(self):
        pass
