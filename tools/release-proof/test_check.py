#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Text-only regressions for the release proof checker; no builds or retail inputs."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
MANIFEST = ROOT / "docs/release-proof/evidence-manifest.json"
SPEC = importlib.util.spec_from_file_location("release_proof", HERE / "check.py")
assert SPEC and SPEC.loader
release_proof = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = release_proof
SPEC.loader.exec_module(release_proof)


def payload() -> dict:
    return json.loads(MANIFEST.read_text(encoding="utf-8"))


def temporary_manifest(value: dict):
    directory = tempfile.TemporaryDirectory()
    path = Path(directory.name) / "manifest.json"
    path.write_text(json.dumps(value), encoding="utf-8")
    return directory, path


class ReleaseProofTests(unittest.TestCase):
    def validate_payload(self, value: dict) -> dict:
        temporary, path = temporary_manifest(value)
        try:
            return release_proof.validate(ROOT, path)
        finally:
            temporary.cleanup()

    def test_current_manifest_is_internally_consistent_but_not_release_ready(self) -> None:
        report = release_proof.validate(ROOT, MANIFEST)
        self.assertTrue(report["ok"])
        self.assertEqual(report["cargo_manifests"], 23)
        self.assertEqual(report["lockfiles"], 7)
        self.assertEqual(report["lock_package_records"], 179)
        self.assertFalse(report["readiness"]["source"])
        self.assertFalse(report["readiness"]["distribution"])
        self.assertIn("source-license-coverage", report["blockers"]["source"])
        self.assertIn(
            "independent-presentation-content", report["blockers"]["distribution"]
        )
        self.assertIn(
            "remote-workspace-license-consistency", report["blockers"]["source"]
        )

    def test_hash_drift_is_refused(self) -> None:
        value = payload()
        value["hash_bound_files"][0]["sha256"] = "0" * 64
        with self.assertRaisesRegex(release_proof.ProofError, "hash drift for LICENSE"):
            self.validate_payload(value)

    def test_uninventoried_cargo_manifest_is_refused(self) -> None:
        value = payload()
        value["repository_cargo_manifests"] = value["repository_cargo_manifests"][:-1]
        with self.assertRaisesRegex(release_proof.ProofError, "Cargo manifest coverage drift"):
            self.validate_payload(value)

    def test_remote_workspace_template_is_in_manifest_coverage(self) -> None:
        value = payload()
        paths = {item["path"] for item in value["repository_cargo_manifests"]}
        self.assertIn("tools/oracle/remote-Cargo.toml", paths)

    def test_license_declaration_drift_is_refused(self) -> None:
        value = payload()
        record = next(
            item
            for item in value["repository_cargo_manifests"]
            if item["path"] == "tools/owned-peer/Cargo.toml"
        )
        record["declared"] = "GPL-3.0-or-later"
        with self.assertRaisesRegex(release_proof.ProofError, "license declaration drift"):
            self.validate_payload(value)

    def test_lock_count_drift_is_refused(self) -> None:
        value = payload()
        value["locked_dependency_sets"][0]["package_records"] += 1
        with self.assertRaisesRegex(release_proof.ProofError, "package-count drift"):
            self.validate_payload(value)

    def test_declared_readiness_cannot_hide_a_blocker(self) -> None:
        value = payload()
        value["declared_readiness"]["source"] = True
        with self.assertRaisesRegex(release_proof.ProofError, "contradicts gates"):
            self.validate_payload(value)

    def test_integer_is_not_accepted_as_boolean_readiness(self) -> None:
        value = payload()
        value["declared_readiness"]["source"] = 0
        with self.assertRaisesRegex(release_proof.ProofError, "fields are invalid"):
            self.validate_payload(value)

    def test_required_gate_cannot_be_deleted(self) -> None:
        value = payload()
        value["gates"] = value["gates"][:-1]
        with self.assertRaisesRegex(release_proof.ProofError, "gate registry drift"):
            self.validate_payload(value)

    def test_required_gate_scope_cannot_be_narrowed(self) -> None:
        value = payload()
        gate = next(
            item for item in value["gates"] if item["id"] == "source-license-coverage"
        )
        gate["scopes"] = ["distribution"]
        with self.assertRaisesRegex(release_proof.ProofError, "scope drift"):
            self.validate_payload(value)

    def test_license_text_digest_cannot_be_relabeled(self) -> None:
        value = payload()
        value["license_text_catalog"][0]["expressions"] = ["MIT"]
        with self.assertRaisesRegex(release_proof.ProofError, "mapping is not recognized"):
            self.validate_payload(value)

    def test_path_alias_is_not_canonical(self) -> None:
        with self.assertRaisesRegex(release_proof.ProofError, "not a canonical"):
            release_proof._relative("docs//install.md", "synthetic path")

    def test_proved_gate_evidence_must_be_hash_bound(self) -> None:
        value = payload()
        value["hash_bound_files"] = [
            item
            for item in value["hash_bound_files"]
            if item["path"] != "tools/release-audit-fixtures/test.sh"
        ]
        with self.assertRaisesRegex(release_proof.ProofError, "unbound evidence"):
            self.validate_payload(value)

    def test_gate_status_cannot_override_derived_license_evidence(self) -> None:
        value = payload()
        gate = next(
            item for item in value["gates"] if item["id"] == "source-license-coverage"
        )
        gate["status"] = "proved"
        gate["blocker"] = None
        value["declared_readiness"]["source"] = True
        with self.assertRaisesRegex(release_proof.ProofError, "contradicts derived evidence"):
            self.validate_payload(value)

    def test_proved_gate_cannot_carry_a_blocker(self) -> None:
        value = payload()
        gate = next(item for item in value["gates"] if item["status"] == "proved")
        gate["blocker"] = "synthetic contradiction"
        with self.assertRaisesRegex(release_proof.ProofError, "unexpectedly carries a blocker"):
            self.validate_payload(value)


if __name__ == "__main__":
    unittest.main()
