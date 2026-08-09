"""Descent of Nations — reinforcement-learning environment.

    from don_env import DonVecEnv
    env = DonVecEnv(num_envs=64, num_agents=2)
    ua, pa = env.sample_masked_actions(np.random.default_rng(0))
    obs, reward, done, trunc, info = env.step(ua, pa)

Read ``env.provenance()`` before you read a training curve: it lists, from the env itself,
which parts of the action space have real dynamics behind them and which are scaffolding.
"""

from ._native import unpack_mask, view
from .core import DonVecEnv, Head
from .spaces import action_space, observation_space

__all__ = [
    "DonVecEnv",
    "Head",
    "action_space",
    "observation_space",
    "unpack_mask",
    "view",
]
__version__ = "0.1.0"
