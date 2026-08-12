#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Text-only regressions for the release proof checker; no builds or retail inputs."""

from __future__ import annotations

import importlib.util
import hashlib
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


def file_record(path: Path) -> dict[str, object]:
    data = path.read_bytes()
    return {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}


class SyntheticDistribution:
    """Small complete release graph made only from synthetic test bytes."""

    def __init__(self, root: Path) -> None:
        self.root = root
        self.payload_root = root / "dist/game"
        for directory in (
            "assets/ui",
            "LICENSES",
            "bin",
            "NOTICES",
            "installer",
        ):
            (self.payload_root / directory).mkdir(parents=True, exist_ok=True)
        (root / "release/installer-results").mkdir(parents=True)

        self.asset = self.payload_root / "assets/ui/cursor.svg"
        self.asset.write_text("<svg><!-- independent synthetic cursor --></svg>\n", encoding="utf-8")
        self.license = self.payload_root / "LICENSES/CC0-1.0.txt"
        self.license.write_text("Synthetic fixture license text.\n" * 16, encoding="utf-8")
        self.attribution = self.payload_root / "ATTRIBUTION.txt"
        self.attribution.write_text("Synthetic Author, CC0-1.0\n", encoding="utf-8")
        self.binary = self.payload_root / "bin/don"
        self.binary.write_bytes(b"synthetic mixed first/third-party product binary\n")
        self.notice = self.payload_root / "NOTICES/synthetic-dep.txt"
        self.notice.write_text("Synthetic dependency notice.\n", encoding="utf-8")
        self.installer = self.payload_root / "installer/setup"
        self.installer.write_bytes(b"synthetic installer\n")

        self.content_manifest = self.payload_root / "independent-content.json"
        self.content = {
            "schema": "don.independent-content.v1",
            "package": {"name": "Synthetic presentation", "version": "1"},
            "asset_roots": ["assets"],
            "licenses": [
                {
                    "id": "CC0-1.0",
                    "path": "LICENSES/CC0-1.0.txt",
                    **file_record(self.license),
                }
            ],
            "assets": [
                {
                    "path": "assets/ui/cursor.svg",
                    "kind": "art",
                    "role": "synthetic cursor",
                    **file_record(self.asset),
                    "author": "Synthetic Author",
                    "source_url": "https://example.invalid/synthetic/cursor",
                    "license": "CC0-1.0",
                    "attribution": "Synthetic Author, CC0-1.0",
                    "modifications": "unmodified synthetic fixture",
                }
            ],
        }
        self.content_manifest.write_text(
            json.dumps(self.content, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )

        categories = {
            "assets/ui/cursor.svg": "independent-content",
            "LICENSES/CC0-1.0.txt": "content-license",
            "ATTRIBUTION.txt": "content-license",
            "bin/don": "mixed-binary",
            "NOTICES/synthetic-dep.txt": "dependency-notice",
            "installer/setup": "installer",
            "independent-content.json": "content-license",
        }
        files = []
        for relative, category in categories.items():
            files.append(
                {
                    "path": relative,
                    **file_record(self.payload_root / relative),
                    "role": f"synthetic {category}",
                    "category": category,
                }
            )
        self.product_payload_path = root / "release/product-payload.json"
        self.product_payload = {
            "schema": "don.release-product-payload.v1",
            "product": "Synthetic DoN",
            "version": "1",
            "payload_root": "dist/game",
            "files": files,
        }
        self.write(self.product_payload_path, self.product_payload)
        self.product = release_proof._validate_product_payload(
            root, "release/product-payload.json", self.product_payload
        )

        auditor = release_proof._load_content_license_auditor()
        audit_report = auditor.audit_release(self.payload_root, self.content_manifest)
        self.clearance_path = root / "release/content-clearance.json"
        self.clearance = {
            "schema": "don.release-content-clearance.v1",
            "payload": "release/product-payload.json",
            "payload_sha256": self.product["artifact_sha256"],
            "content_manifest": "dist/game/independent-content.json",
            "audit_report": audit_report,
            "human_review": {
                "reviewer": "Synthetic Release Reviewer",
                "reviewed_on": "2026-08-09",
                "decision": "approved-for-distribution",
                "manifest_sha256": audit_report["manifest_sha256"],
                "scope": "synthetic fixture rights and attribution review",
            },
            "attribution_notice": "ATTRIBUTION.txt",
        }
        self.write(self.clearance_path, self.clearance)

        self.lock_path = root / "Cargo.lock"
        self.lock_path.write_text(
            """version = 3

[[package]]
name = "synthetic-dep"
version = "1.0.0"
source = "registry+https://example.invalid/index"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
""",
            encoding="utf-8",
        )
        identity = {
            "name": "synthetic-dep",
            "version": "1.0.0",
            "source": "registry+https://example.invalid/index",
            "checksum": "a" * 64,
        }
        self.notices_path = root / "release/product-dependency-notices.json"
        self.notices = {
            "schema": "don.product-dependency-notices.v1",
            "payload": "release/product-payload.json",
            "payload_sha256": self.product["artifact_sha256"],
            "locks": [{"path": "Cargo.lock", "sha256": release_proof._sha256(self.lock_path)}],
            "artifacts": [
                {"payload_path": "bin/don", "lockfile": "Cargo.lock", "packages": [identity]}
            ],
            "packages": [
                {
                    "lockfile": "Cargo.lock",
                    **identity,
                    "license_expression": "MIT",
                    "license_evidence": ["NOTICES/synthetic-dep.txt"],
                    "required_notice_paths": ["NOTICES/synthetic-dep.txt"],
                    "source_provision": "source offer recorded in the synthetic notice",
                }
            ],
        }
        self.write(self.notices_path, self.notices)

        results = {
            "install": {"managed_payload_matches": True, "launch_probe_passed": True},
            "configure": {"configuration_roundtrip_matches": True},
            "repair": {
                "tamper_injected": True,
                "tamper_detected": True,
                "managed_payload_matches": True,
            },
            "remove": {"managed_paths_remaining": [], "user_data_policy_matches": True},
        }
        self.operations = []
        self.hash_records: dict[str, dict[str, object]] = {}
        for action, result in results.items():
            relative = f"release/installer-results/linux-{action}.json"
            path = root / relative
            self.write(
                path,
                {
                    "schema": "don.installer-operation-result.v1",
                    "payload_sha256": self.product["artifact_sha256"],
                    "platform": "linux-x86_64",
                    "action": action,
                    "exit_code": 0,
                    "result": result,
                },
            )
            self.hash_records[relative] = {
                "path": relative,
                "sha256": release_proof._sha256(path),
                "role": f"synthetic installer {action} result",
            }
            self.operations.append(
                {"platform": "linux-x86_64", "action": action, "report": relative}
            )
        self.installer_evidence_path = root / "release/installer-evidence.json"
        self.installer_evidence = {
            "schema": "don.release-installer-evidence.v1",
            "payload": "release/product-payload.json",
            "payload_sha256": self.product["artifact_sha256"],
            "installers": ["installer/setup"],
            "operations": self.operations,
        }
        self.write(self.installer_evidence_path, self.installer_evidence)

        for relative in (
            "release/product-payload.json",
            "release/content-clearance.json",
            "release/product-dependency-notices.json",
            "release/installer-evidence.json",
        ):
            self.hash_records[relative] = {
                "path": relative,
                "sha256": release_proof._sha256(root / relative),
                "role": "synthetic completion artifact",
            }

    @staticmethod
    def write(path: Path, value: dict) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    def gates(self) -> dict[str, dict]:
        paths = {
            "assembled-product-packaging": "release/product-payload.json",
            "binary-third-party-notices": "release/product-dependency-notices.json",
            "independent-presentation-content": "release/content-clearance.json",
            "standalone-product-installer": "release/installer-evidence.json",
        }
        all_paths = list(paths.values())
        return {
            gate_id: {"evidence": all_paths if gate_id == "assembled-product-packaging" else [path]}
            for gate_id, path in paths.items()
        }


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
        self.assertEqual(report["cargo_manifests"], 25)
        self.assertEqual(report["lockfiles"], 9)
        self.assertEqual(report["lock_package_records"], 184)
        self.assertFalse(report["readiness"]["source"])
        self.assertFalse(report["readiness"]["distribution"])
        self.assertEqual(
            report["source_archive_reproducibility"]["outcome"],
            "reproduced-byte-identical-candidate",
        )
        self.assertTrue(
            report["source_archive_reproducibility"]["candidate_reproduced"]
        )
        self.assertGreater(
            report["source_archive_reproducibility"]["don_sim_lib_tests"]["passed"],
            1000,
        )
        self.assertFalse(
            report["source_archive_reproducibility"]["whole_archive_workspace_tests_proven"]
        )
        self.assertIn("source-license-coverage", report["blockers"]["source"])
        self.assertIn(
            "independent-presentation-content", report["blockers"]["distribution"]
        )
        self.assertIn(
            "remote-workspace-license-consistency", report["blockers"]["source"]
        )
        self.assertNotIn("source-archive-reproducibility", report["blockers"]["source"])
        self.assertNotIn(
            "retail-controller-byte-restoration", report["blockers"]["distribution"]
        )
        self.assertNotIn(
            "controller-stop-incident-closure", report["blockers"]["distribution"]
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

    def test_discovery_prunes_ignored_build_trees(self) -> None:
        with tempfile.TemporaryDirectory(prefix="don-release-discovery-") as directory:
            root = Path(directory)
            (root / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
            ignored = root / "target/deep"
            ignored.mkdir(parents=True)
            (ignored / "Cargo.lock").write_text("not repository evidence\n", encoding="utf-8")
            (ignored / "Cargo.toml").write_text("not repository evidence\n", encoding="utf-8")
            self.assertEqual(release_proof._discover(root, "Cargo.lock"), {"Cargo.lock"})
            self.assertEqual(release_proof._discover_cargo_manifests(root), set())

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

    def test_valid_controller_lifecycle_artifact_cannot_be_declared_blocked(self) -> None:
        value = payload()
        gate = next(
            item
            for item in value["gates"]
            if item["id"] == "retail-controller-byte-restoration"
        )
        gate["status"] = "blocked"
        gate["blocker"] = "synthetic stale blocker"
        with self.assertRaisesRegex(
            release_proof.ProofError, "contradicts validated retail-control artifacts"
        ):
            self.validate_payload(value)

    def test_valid_incident_closure_artifact_cannot_be_declared_blocked(self) -> None:
        value = payload()
        gate = next(
            item
            for item in value["gates"]
            if item["id"] == "controller-stop-incident-closure"
        )
        gate["status"] = "blocked"
        gate["blocker"] = "synthetic stale blocker"
        with self.assertRaisesRegex(
            release_proof.ProofError, "contradicts validated retail-control artifacts"
        ):
            self.validate_payload(value)


class DistributionArtifactContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="don-release-artifact-test-")
        self.root = Path(self.temporary.name).resolve()
        self.release = SyntheticDistribution(self.root)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_complete_synthetic_distribution_graph_is_semantically_proved(self) -> None:
        completed = release_proof._validate_distribution_artifacts(
            self.root, self.release.gates(), self.release.hash_records
        )
        self.assertEqual(
            completed,
            {
                "assembled-product-packaging": True,
                "binary-third-party-notices": True,
                "independent-presentation-content": True,
                "standalone-product-installer": True,
            },
        )

    def test_unmanifested_payload_file_is_refused(self) -> None:
        (self.release.payload_root / "hidden.bin").write_bytes(b"unmanifested payload\n")
        with self.assertRaisesRegex(release_proof.ProofError, "payload coverage drift"):
            release_proof._validate_product_payload(
                self.root,
                "release/product-payload.json",
                self.release.product_payload,
            )

    def test_content_clearance_requires_an_affirmative_human_decision(self) -> None:
        self.release.clearance["human_review"]["decision"] = "pending"
        with self.assertRaisesRegex(release_proof.ProofError, "not approved"):
            release_proof._validate_content_clearance(
                self.root,
                "release/content-clearance.json",
                self.release.clearance,
                self.release.product,
            )

    def test_content_clearance_cannot_hide_an_unattributed_asset(self) -> None:
        self.release.attribution.write_text("Different attribution\n", encoding="utf-8")
        product_record = next(
            record
            for record in self.release.product_payload["files"]
            if record["path"] == "ATTRIBUTION.txt"
        )
        product_record.update(file_record(self.release.attribution))
        self.release.write(self.release.product_payload_path, self.release.product_payload)
        product = release_proof._validate_product_payload(
            self.root, "release/product-payload.json", self.release.product_payload
        )
        self.release.clearance["payload_sha256"] = product["artifact_sha256"]
        with self.assertRaisesRegex(release_proof.ProofError, "omits manifest attribution"):
            release_proof._validate_content_clearance(
                self.root,
                "release/content-clearance.json",
                self.release.clearance,
                product,
            )

    def test_dependency_identity_must_exist_in_the_exact_lock(self) -> None:
        self.release.notices["artifacts"][0]["packages"][0]["checksum"] = "b" * 64
        with self.assertRaisesRegex(release_proof.ProofError, "absent from exact lock"):
            release_proof._validate_product_notices(
                self.root,
                "release/product-dependency-notices.json",
                self.release.notices,
                self.release.product,
            )

    def test_dependency_bearing_payload_cannot_be_left_uncovered(self) -> None:
        self.release.notices["artifacts"] = []
        with self.assertRaisesRegex(release_proof.ProofError, "artifacts must be non-empty"):
            release_proof._validate_product_notices(
                self.root,
                "release/product-dependency-notices.json",
                self.release.notices,
                self.release.product,
            )

    def test_installer_requires_install_configure_repair_and_remove(self) -> None:
        self.release.installer_evidence["operations"] = [
            operation
            for operation in self.release.operations
            if operation["action"] != "repair"
        ]
        with self.assertRaisesRegex(release_proof.ProofError, "coverage is incomplete"):
            release_proof._validate_installer_evidence(
                self.root,
                "release/installer-evidence.json",
                self.release.installer_evidence,
                self.release.product,
                self.release.hash_records,
            )

    def test_installer_success_claim_must_match_hash_bound_operation_result(self) -> None:
        operation = next(
            operation for operation in self.release.operations if operation["action"] == "repair"
        )
        report_path = self.root / operation["report"]
        report = json.loads(report_path.read_text(encoding="utf-8"))
        report["result"]["tamper_detected"] = False
        self.release.write(report_path, report)
        self.release.hash_records[operation["report"]]["sha256"] = release_proof._sha256(
            report_path
        )
        with self.assertRaisesRegex(release_proof.ProofError, "did not prove successful repair"):
            release_proof._validate_installer_evidence(
                self.root,
                "release/installer-evidence.json",
                self.release.installer_evidence,
                self.release.product,
                self.release.hash_records,
            )

    def test_blocked_report_names_expected_completion_artifacts(self) -> None:
        report = release_proof.validate(ROOT, MANIFEST)
        independent = next(
            item
            for item in report["blocker_details"]["distribution"]
            if item["id"] == "independent-presentation-content"
        )
        self.assertEqual(independent["expected_artifact"], "release/content-clearance.json")
        self.assertIn("no real non-empty independent pack", independent["reason"])


if __name__ == "__main__":
    unittest.main()
