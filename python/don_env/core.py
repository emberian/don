"""`DonVecEnv` — the batched core every other front end wraps.

This is the object that owns the native env. `gym_vector.DonGymVectorEnv` gives it
Gymnasium `VectorEnv` semantics and `pettingzoo_env.DonParallelEnv` gives it PettingZoo
`ParallelEnv` semantics; both are thin, because the batched-multi-agent shape is the
primitive and the single-agent and per-agent-dict shapes are views of it.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import numpy as np

from ._native import load_native, unpack_mask, view


@dataclass(frozen=True)
class Head:
    """One factored action head: its name, its size, and where its mask lives."""

    name: str
    size: int
    mask_offset: int

    def unpack(self, packed: np.ndarray) -> np.ndarray:
        return unpack_mask(packed, self.mask_offset, self.size)


class DonVecEnv:
    """N independent Descent of Nations worlds, stepped together.

    Shapes, with ``E`` = ``num_envs``, ``A`` = ``num_agents``, ``C`` = ``max_controlled``,
    ``N`` = ``max_entities``:

    ==================  ==========================================  =========
    array               shape                                       dtype
    ==================  ==========================================  =========
    ``spatial``         ``(E, A, planes, grid_h, grid_w)``          float32
    ``entities``        ``(E, A, N, entity_features)``              float32
    ``globals``         ``(E, A, global_features)``                 float32
    ``unit_masks``      ``(E, A, C, unit_mask_record_bytes)``       uint8 (packed)
    ``player_masks``    ``(E, A, player_mask_record_bytes)``        uint8 (packed)
    ``rewards``         ``(E, A)``                                  float32
    ``reward_terms``    ``(E, A, 20)``                              float32
    ``dones``           ``(E,)``                                    uint8
    ``truncateds``      ``(E,)``                                    uint8
    ``entity_rows``     ``(E, A, N)``                               int32
    ==================  ==========================================  =========

    Unit actions are ``(E, A, C, 10)`` int32; player actions are ``(E, A, 5)`` int32.
    Every array is a view into Rust memory and is invalidated by the next :meth:`step`.
    """

    def __init__(self, num_envs: int = 8, **kwargs: Any):
        native = load_native()
        self._env = native.VecEnv(num_envs, **kwargs)
        self.spec: dict[str, Any] = dict(self._env.spec())
        self.num_envs = num_envs
        self.num_agents = self.spec["num_agents"]
        self.max_controlled = self.spec["max_controlled"]
        self.max_entities = self.spec["max_entities"]

        self.unit_heads = [
            Head(n, s, o)
            for n, s, o in zip(
                self.spec["unit_head_names"],
                self.spec["unit_head_sizes"],
                self.spec["unit_mask_offsets"],
            )
        ]
        self.player_heads = [
            Head(n, s, o)
            for n, s, o in zip(
                self.spec["player_head_names"],
                self.spec["player_head_sizes"],
                self.spec["player_mask_offsets"],
            )
        ]
        self.unit_nvec = np.array([h.size for h in self.unit_heads], dtype=np.int64)
        self.player_nvec = np.array([h.size for h in self.player_heads], dtype=np.int64)
        self.agents = list(self.spec["agent_ids"])
        self._refresh()

    # -- buffers -----------------------------------------------------------------
    def _refresh(self) -> None:
        self._buf = {k: view(*v) for k, v in self._env.buffers().items()}

    def __getattr__(self, name: str) -> np.ndarray:
        buf = self.__dict__.get("_buf")
        if buf is not None and name in buf:
            return buf[name]
        raise AttributeError(name)

    @property
    def buffers(self) -> dict[str, np.ndarray]:
        return self._buf

    # -- lifecycle ---------------------------------------------------------------
    def reset(self):
        self._env.reset()
        self._refresh()
        return self.observation()

    def step(self, unit_actions: np.ndarray, player_actions: np.ndarray):
        """Apply actions and advance one env step (`frames_per_step` sim frames).

        Both arrays must be C-contiguous int32; they are read in place, so passing the
        sampler's own buffers costs nothing.
        """
        if unit_actions.dtype != np.int32 or not unit_actions.flags.c_contiguous:
            unit_actions = np.ascontiguousarray(unit_actions, dtype=np.int32)
        if player_actions.dtype != np.int32 or not player_actions.flags.c_contiguous:
            player_actions = np.ascontiguousarray(player_actions, dtype=np.int32)
        self._env.step(unit_actions, player_actions)
        return (
            self.observation(),
            self._buf["rewards"],
            self._buf["dones"].astype(bool),
            self._buf["truncateds"].astype(bool),
            self.info(),
        )

    def observation(self) -> dict[str, np.ndarray]:
        return {
            "spatial": self._buf["spatial"],
            "entities": self._buf["entities"],
            "globals": self._buf["globals"],
            "unit_mask": self._buf["unit_masks"],
            "player_mask": self._buf["player_masks"],
        }

    def info(self) -> dict[str, Any]:
        return {
            "reward_terms": self._buf["reward_terms"],
            "entity_rows": self._buf["entity_rows"],
        }

    # -- masks -------------------------------------------------------------------
    def unpack_unit_masks(self) -> list[np.ndarray]:
        """One bool array per unit head, shape `(E, A, C, size)`. Allocates."""
        return [h.unpack(self._buf["unit_masks"]) for h in self.unit_heads]

    def unpack_player_masks(self) -> list[np.ndarray]:
        return [h.unpack(self._buf["player_masks"]) for h in self.player_heads]

    def sample_masked_actions_native(self):
        """Uniform sample under the mask, done in Rust, straight over the packed bitsets.

        Returns views into the env's own buffers, so it allocates nothing and can be fed
        directly back into :meth:`step`. Use this for scripted opponents and for any
        throughput number that is supposed to be about the environment: the numpy version
        below spends an order of magnitude more time unpacking the 806-wide ``Type`` head
        than the env spends simulating.
        """
        self._env.sample_masked()
        return self._buf["sampled_unit_actions"], self._buf["sampled_player_actions"]

    def sample_masked_actions(self, rng: np.random.Generator):
        """Uniform sample under the mask, in numpy — the shape a real policy has.

        Kept because it is what a torch policy's sampling path looks like, and because
        comparing it against :meth:`sample_masked_actions_native` is how the cost of mask
        unpacking was measured. Prefer the native one unless you are testing this path.
        """
        ua = np.empty(
            (self.num_envs, self.num_agents, self.max_controlled, len(self.unit_heads)),
            dtype=np.int32,
        )
        packed = self._buf["unit_masks"]
        for i, h in enumerate(self.unit_heads):
            bits = h.unpack(packed)
            keys = rng.random(bits.shape, dtype=np.float32)
            keys *= bits
            ua[..., i] = np.argmax(keys, axis=-1)
        pa = np.empty((self.num_envs, self.num_agents, len(self.player_heads)), dtype=np.int32)
        ppacked = self._buf["player_masks"]
        for i, h in enumerate(self.player_heads):
            bits = h.unpack(ppacked)
            keys = rng.random(bits.shape, dtype=np.float32)
            keys *= bits
            pa[..., i] = np.argmax(keys, axis=-1)
        return ua, pa

    def noop_actions(self):
        ua = np.zeros(
            (self.num_envs, self.num_agents, self.max_controlled, len(self.unit_heads)),
            dtype=np.int32,
        )
        pa = np.zeros((self.num_envs, self.num_agents, len(self.player_heads)), dtype=np.int32)
        return ua, pa

    # -- reward ------------------------------------------------------------------
    def set_reward_weights(self, **weights: float) -> None:
        """Shaping is a weight vector over the named terms in ``spec['reward_terms']``.

        Default is sparse: ``win=+1, loss=-1``. Nothing else is weighted, because this
        project has no measurement that would justify a shaping prior.
        """
        for k, v in weights.items():
            self._env.set_reward_weight(k, float(v))

    @property
    def reward_weights(self) -> dict[str, float]:
        return dict(self._env.reward_weights())

    def scores(self) -> np.ndarray:
        """`(E, A, 12)` — the eleven `LeaderData` score fields plus our unweighted total.

        The eleven components are the engine's; the total is *ours*, because
        `Leader::compute_score` (0x006EC560) has not been read and its weighting is
        underived.
        """
        return np.asarray(self._env.scores(), dtype=np.int32)

    # -- honesty -----------------------------------------------------------------
    def provenance(self) -> list[tuple[str, str]]:
        return [tuple(x) for x in self._env.provenance()]

    def unimplemented(self) -> list[tuple[str, int]]:
        """Verbs the policy emitted that this build has no dynamics for."""
        return [tuple(x) for x in self._env.unimplemented()]

    def apply_stats(self) -> dict[str, int]:
        """Cumulative outcome counts for every action applied.

        ``illegal`` must stay 0 for a policy that samples under the mask; if it is not,
        either the sampler ignored the mask or a mask is wrong.
        """
        return dict(self._env.apply_stats())

    @property
    def steps_taken(self) -> int:
        return self._env.steps_taken
