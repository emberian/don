"""Loading the native module, and building numpy views over Rust-owned memory.

The env allocates every observation, mask and reward array once and rewrites it in place.
Python never receives an array by value: it receives a pointer, a shape and a dtype, and
wraps them. `_view` is the whole of the zero-copy mechanism.

The lifetime rule that buys it: a view is valid until the next `step`. Keep nothing
without copying.
"""

from __future__ import annotations

import importlib.util
import os
import sys
from pathlib import Path

import numpy as np

_TYPESTR = {
    "float32": "<f4",
    "float64": "<f8",
    "uint8": "|u1",
    "int32": "<i4",
    "int64": "<i8",
}


class _Alias:
    """Minimal `__array_interface__` provider: numpy builds an array *aliasing* `ptr`.

    Deliberately not `np.ctypeslib.as_array`: a ctypes-backed array exports its buffer
    with the byte-order-qualified format ``<i`` rather than ``i``, and PyO3's typed
    `PyBuffer` rejects that, so an action array built one way could not be handed back to
    `step`. This path produces an ordinary numpy array with an ordinary format.
    """

    __slots__ = ("__array_interface__",)

    def __init__(self, ptr: int, shape: tuple[int, ...], typestr: str):
        self.__array_interface__ = {
            "data": (int(ptr), False),
            "shape": shape,
            "typestr": typestr,
            "version": 3,
        }


def _candidate_paths() -> list[Path]:
    here = Path(__file__).resolve()
    pkg = here.parent
    root = pkg.parent.parent  # <repo>/python/don_env/_native.py -> <repo>
    names = ["_don_env.so", "_don_env.pyd", "libdon_env.dylib", "libdon_env.so", "don_env.dll"]
    dirs = [pkg, root / "target" / "release", root / "target" / "debug"]
    if "DON_ENV_LIB" in os.environ:
        dirs.insert(0, Path(os.environ["DON_ENV_LIB"]).parent)
    out = []
    if "DON_ENV_LIB" in os.environ:
        out.append(Path(os.environ["DON_ENV_LIB"]))
    for d in dirs:
        for n in names:
            out.append(d / n)
    return out


def load_native():
    """Import the compiled `_don_env` module, wherever `python/build.sh` put it."""
    try:
        import _don_env  # type: ignore

        return _don_env
    except ImportError:
        pass
    for p in _candidate_paths():
        if not p.exists():
            continue
        spec = importlib.util.spec_from_file_location("_don_env", p)
        if spec is None or spec.loader is None:
            continue
        mod = importlib.util.module_from_spec(spec)
        sys.modules["_don_env"] = mod
        spec.loader.exec_module(mod)
        return mod
    raise ImportError(
        "don_env native module not found. Build it with:\n"
        "    bash python/build.sh\n"
        "or set DON_ENV_LIB to the built library."
    )


def view(ptr: int, shape, dtype: str) -> np.ndarray:
    """A numpy array *aliasing* `ptr`. No copy, no ownership."""
    shape = tuple(int(s) for s in shape)
    n = 1
    for s in shape:
        n *= s
    if n == 0:
        return np.empty(shape, dtype=np.dtype(dtype))
    return np.asarray(_Alias(ptr, shape, _TYPESTR[dtype]))


def unpack_mask(packed: np.ndarray, offset: int, size: int) -> np.ndarray:
    """Expand one bit-packed head to a boolean array `(..., size)`.

    Masks are stored bit-packed LSB-first because the `Type` head alone is 806 wide;
    unpacking is the caller's choice, so a trainer that can consume the packed form on the
    GPU never pays for this.
    """
    nbytes = (size + 7) // 8
    sub = packed[..., offset : offset + nbytes]
    bits = np.unpackbits(sub, axis=-1, bitorder="little")
    return bits[..., :size].astype(bool)
