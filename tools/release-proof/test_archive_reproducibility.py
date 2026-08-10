#!/usr/bin/env python3
"""Regressions for the exact source-archive build blocker artifact."""

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

    def test_current_negative_artifact_verifies(self) -> None:
        report = archive_reproducibility.verify(ROOT, ARTIFACT)
        self.assertTrue(report["ok"])
        self.assertFalse(report["candidate_reproduced"])
        self.assertEqual(report["outcome"], "blocked-missing-exported-build-input")

    def test_positive_reproducibility_claim_is_refused(self) -> None:
        directory, path = self.mutated_artifact(
            lambda value: value["claims"].update(source_archive_buildable=True)
        )
        try:
            with self.assertRaisesRegex(
                archive_reproducibility.ReproducibilityError, "overstate"
            ):
                archive_reproducibility.verify(ROOT, path)
        finally:
            directory.cleanup()

    def test_required_input_hash_drift_is_refused(self) -> None:
        directory, path = self.mutated_artifact(
            lambda value: value["archive_projection"]["required_input"].update(
                sha256="0" * 64
            )
        )
        try:
            with self.assertRaisesRegex(
                archive_reproducibility.ReproducibilityError, "identity"
            ):
                archive_reproducibility.verify(ROOT, path)
        finally:
            directory.cleanup()


if __name__ == "__main__":
    unittest.main()
