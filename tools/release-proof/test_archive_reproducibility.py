#!/usr/bin/env python3
"""Regressions for exact source-archive-to-product-Wasm linkage."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
ARTIFACT = ROOT / "release/source-archive-reproducibility.json"
SPEC = importlib.util.spec_from_file_location(
    "archive_reproducibility", HERE / "archive_reproducibility.py"
)
assert SPEC and SPEC.loader
archive_reproducibility = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = archive_reproducibility
SPEC.loader.exec_module(archive_reproducibility)


class ArchiveReproducibilityTest(unittest.TestCase):
    def mutated_artifact(self, mutate):
        value = json.loads(ARTIFACT.read_text(encoding="utf-8"))
        mutate(value)
        directory = tempfile.TemporaryDirectory(prefix="don-archive-proof-test-")
        path = Path(directory.name) / "artifact.json"
        path.write_text(json.dumps(value), encoding="utf-8")
        return directory, path

    def test_current_product_wasm_linkage_verifies(self) -> None:
        report = archive_reproducibility.verify(ROOT, ARTIFACT)
        self.assertTrue(report["ok"])
        self.assertTrue(report["candidate_reproduced"])
        self.assertFalse(report["live_input_present"])
        self.assertGreater(report["don_sim_lib_tests"]["passed"], 1000)
        self.assertFalse(report["whole_archive_workspace_tests_proven"])
        self.assertGreater(report["source_inputs"], 100)

    def test_whole_workspace_overclaim_is_refused(self) -> None:
        directory, path = self.mutated_artifact(
            lambda value: value["claims"].update(whole_archive_workspace_tests_proven=True)
        )
        try:
            with self.assertRaisesRegex(
                archive_reproducibility.ReproducibilityError, "claims contradict"
            ):
                archive_reproducibility.verify(ROOT, path)
        finally:
            directory.cleanup()

    def test_build_source_hash_drift_is_refused(self) -> None:
        directory, path = self.mutated_artifact(
            lambda value: value["build"]["source_inputs"][0].update(sha256="0" * 64)
        )
        try:
            with self.assertRaisesRegex(
                archive_reproducibility.ReproducibilityError, "source-input drift"
            ):
                archive_reproducibility.verify(ROOT, path)
        finally:
            directory.cleanup()

    def test_candidate_linkage_drift_is_refused(self) -> None:
        directory, path = self.mutated_artifact(
            lambda value: value["build"]["output"].update(sha256="0" * 64)
        )
        try:
            with self.assertRaisesRegex(
                archive_reproducibility.ReproducibilityError, "does not match"
            ):
                archive_reproducibility.verify(ROOT, path)
        finally:
            directory.cleanup()


if __name__ == "__main__":
    unittest.main()
