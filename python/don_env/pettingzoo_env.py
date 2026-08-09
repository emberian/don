"""PettingZoo `ParallelEnv` semantics over `DonVecEnv`.

Two flavours, because self-play at scale wants the second one:

* :class:`DonParallelEnv` — a single game, `agents = ['player_0', ...]`, dict in / dict
  out. This is the conformance-testable shape.
* :class:`DonVectorParallelEnv` — the same agent-keyed interface but each value is
  batched over `num_envs`. Nothing is unbatched, so a league trainer keeps the whole batch
  on one policy forward pass.

Both are views of the same buffers; neither copies observations.
"""

from __future__ import annotations

from typing import Any

import numpy as np

from .core import DonVecEnv
from .spaces import action_space, observation_space

try:  # pragma: no cover
    from pettingzoo import ParallelEnv as _PZBase

    _BASE = _PZBase
except ImportError:  # pragma: no cover
    class _BASE:  # type: ignore[no-redef]
        pass


class DonVectorParallelEnv(_BASE):
    """Agent-keyed, batched over `num_envs`."""

    metadata = {"name": "descent_of_nations_v0", "is_parallelizable": True}

    def __init__(self, num_envs: int = 8, **kw: Any):
        self.core = DonVecEnv(num_envs=num_envs, **kw)
        self.possible_agents = list(self.core.agents)
        self.agents = list(self.possible_agents)
        self._obs_space = observation_space(self.core.spec)
        self._act_space = action_space(self.core.spec)

    def observation_space(self, agent: str):
        return self._obs_space

    def action_space(self, agent: str):
        return self._act_space

    def _split(self, obs: dict[str, np.ndarray]) -> dict[str, dict[str, np.ndarray]]:
        return {a: {k: v[:, i] for k, v in obs.items()} for i, a in enumerate(self.agents)}

    def reset(self, seed: int | None = None, options: dict | None = None):
        self.agents = list(self.possible_agents)
        obs = self.core.reset()
        return self._split(obs), {a: {} for a in self.agents}

    def step(self, actions: dict[str, dict[str, np.ndarray]]):
        ua, pa = self.core.noop_actions()
        for i, a in enumerate(self.possible_agents):
            act = actions.get(a)
            if act is None:
                continue
            ua[:, i] = np.asarray(act["unit"], dtype=np.int32)
            pa[:, i] = np.asarray(act["player"], dtype=np.int32)
        obs, rewards, dones, truncs, info = self.core.step(ua, pa)
        obs_d = self._split(obs)
        rew = {a: rewards[:, i].copy() for i, a in enumerate(self.possible_agents)}
        term = {a: dones.copy() for a in self.possible_agents}
        trunc = {a: truncs.copy() for a in self.possible_agents}
        infos = {
            a: {"reward_terms": info["reward_terms"][:, i]}
            for i, a in enumerate(self.possible_agents)
        }
        return obs_d, rew, term, trunc, infos

    def close(self):
        pass


class DonParallelEnv(DonVectorParallelEnv):
    """One game, unbatched — the classic PettingZoo shape."""

    def __init__(self, **kw: Any):
        super().__init__(num_envs=1, **kw)

    def _split(self, obs):
        return {a: {k: v[0, i] for k, v in obs.items()} for i, a in enumerate(self.agents)}

    def reset(self, seed: int | None = None, options: dict | None = None):
        o, i = super().reset(seed, options)
        return o, i

    def step(self, actions):
        obs, rew, term, trunc, infos = super().step(
            {a: {"unit": v["unit"][None], "player": v["player"][None]} for a, v in actions.items()}
        )
        rew = {a: float(v[0]) for a, v in rew.items()}
        term = {a: bool(v[0]) for a, v in term.items()}
        trunc = {a: bool(v[0]) for a, v in trunc.items()}
        if any(term.values()) or any(trunc.values()):
            self.agents = []
        return obs, rew, term, trunc, infos
