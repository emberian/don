"""Gymnasium space construction, with a dependency-free fallback.

`gymnasium` is optional: the env is usable without it, and the smoke test does not need
it. When it is installed these build the real space objects; when it is not, they build
small stand-ins with the same `.shape` / `.nvec` / `.sample()` surface so nothing
downstream has to branch.
"""

from __future__ import annotations

import numpy as np

try:  # pragma: no cover - exercised only when gymnasium is installed
    import gymnasium as gym
    from gymnasium import spaces as gspaces

    HAVE_GYM = True
except ImportError:  # pragma: no cover
    gym = None
    gspaces = None
    HAVE_GYM = False


class _Box:
    def __init__(self, low, high, shape, dtype=np.float32):
        self.low, self.high, self.shape, self.dtype = low, high, tuple(shape), dtype

    def sample(self, rng=None):
        rng = rng or np.random.default_rng()
        return rng.random(self.shape, dtype=np.float32)

    def __repr__(self):
        return f"Box({self.low}, {self.high}, {self.shape}, {self.dtype.__name__})"


class _MultiDiscrete:
    def __init__(self, nvec):
        self.nvec = np.asarray(nvec, dtype=np.int64)
        self.shape = self.nvec.shape

    def sample(self, rng=None):
        rng = rng or np.random.default_rng()
        return (rng.random(self.shape) * self.nvec).astype(np.int32)

    def __repr__(self):
        return f"MultiDiscrete({self.nvec.tolist()})"


class _Dict(dict):
    def __repr__(self):
        return "Dict(" + ", ".join(f"{k}: {v}" for k, v in self.items()) + ")"


def box(low, high, shape, dtype=np.float32):
    if HAVE_GYM:
        return gspaces.Box(low=low, high=high, shape=tuple(shape), dtype=dtype)
    return _Box(low, high, shape, dtype)


def multi_discrete(nvec):
    if HAVE_GYM:
        return gspaces.MultiDiscrete(np.asarray(nvec, dtype=np.int64))
    return _MultiDiscrete(nvec)


def dict_space(**kw):
    if HAVE_GYM:
        return gspaces.Dict(kw)
    return _Dict(kw)


def observation_space(spec: dict):
    """Per-agent observation space, matching `DonVecEnv.observation()` minus the batch
    and agent axes."""
    gw, gh = spec["grid"]
    planes = len(spec["spatial_planes"])
    return dict_space(
        spatial=box(0.0, np.inf, (planes, gh, gw)),
        entities=box(-np.inf, np.inf, (spec["max_entities"], len(spec["entity_features"]))),
        globals=box(-np.inf, np.inf, (len(spec["global_features"]),)),
        unit_mask=box(0, 255, (spec["max_controlled"], spec["unit_mask_record_bytes"]), np.uint8),
        player_mask=box(0, 255, (spec["player_mask_record_bytes"],), np.uint8),
    )


def action_space(spec: dict):
    """Per-agent action space: a `MultiDiscrete` per controlled entity, plus the global
    player heads.

    Deliberately **not** flattened. The engine's own parameterisation is
    ``(verb, target, ...)``; collapsing it into one categorical of size
    ``prod(head_sizes)`` would be ~10^13 wide and, worse, would destroy the sharing between
    ``Attack(target=i)`` and ``Attack(target=j)``.
    """
    unit = np.tile(np.asarray(spec["unit_head_sizes"], dtype=np.int64), (spec["max_controlled"], 1))
    return dict_space(
        unit=multi_discrete(unit),
        player=multi_discrete(np.asarray(spec["player_head_sizes"], dtype=np.int64)),
    )
