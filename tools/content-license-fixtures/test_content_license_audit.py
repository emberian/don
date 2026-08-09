#!/usr/bin/env python3
"""Synthetic regression suite for content-license-audit.py; no retail assets are used."""

from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


HERE = Path(__file__).resolve().parent
TOOL = HERE.parent / "content-license-audit.py"
SPEC = importlib.util.spec_from_file_location("content_license_audit", TOOL)
assert SPEC is not None and SPEC.loader is not None
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


def record(path: Path) -> dict[str, object]:
    payload = path.read_bytes()
    return {"size": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}


class SyntheticRelease:
    def __init__(self, root: Path) -> None:
        self.root = root
        (root / "assets/ui").mkdir(parents=True)
        (root / "LICENSES").mkdir()
        self.asset = root / "assets/ui/cursor.svg"
        self.asset.write_text("<svg><!-- synthetic --></svg>\n", encoding="utf-8")
        self.license = root / "LICENSES/CC0-1.0.txt"
        self.license.write_text(
            "Synthetic CC0 fixture text; not a license copy.\n" * 8, encoding="utf-8"
        )
        self.manifest = root / "independent-content.json"
        self.payload = {
            "schema": "don.independent-content.v1",
            "package": {"name": "Synthetic presentation", "version": "1"},
            "asset_roots": ["assets"],
            "licenses": [
                {"id": "CC0-1.0", "path": "LICENSES/CC0-1.0.txt", **record(self.license)}
            ],
            "assets": [
                {
                    "path": "assets/ui/cursor.svg",
                    "kind": "art",
                    "role": "synthetic cursor",
                    **record(self.asset),
                    "author": "Synthetic Fixture Author",
                    "source_url": "https://example.invalid/don-fixture/cursor",
                    "license": "CC0-1.0",
                    "attribution": "Synthetic Fixture Author, CC0-1.0",
                    "modifications": "unmodified synthetic fixture",
                }
            ],
        }
        self.write_manifest()

    def write_manifest(self) -> None:
        self.manifest.write_text(
            json.dumps(self.payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )


class ContentLicenseAuditTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="don-content-license-test-")
        self.root = Path(self.temp.name)
        self.release = SyntheticRelease(self.root)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def assert_refuses(self, needle: str) -> None:
        with self.assertRaisesRegex(audit.AuditError, needle):
            audit.audit_release(self.root, self.release.manifest)

    def test_complete_synthetic_manifest_passes_with_narrow_claim(self) -> None:
        report = audit.audit_release(self.root, self.release.manifest)
        self.assertTrue(report["ready"])
        self.assertFalse(report["legal_title_certified"])
        self.assertEqual(report["assets"], 1)
        self.assertEqual(report["assets_by_kind"], {"art": 1})

    def test_unmanifested_file_is_refused(self) -> None:
        (self.root / "assets/ui/extra.png").write_bytes(b"synthetic extra")
        self.assert_refuses("unmanifested files")

    def test_changed_asset_bytes_are_refused(self) -> None:
        self.release.asset.write_text("changed after review\n", encoding="utf-8")
        self.assert_refuses("asset bytes do not match manifest")

    def test_changed_license_text_is_refused(self) -> None:
        self.release.license.write_text("changed policy text\n" * 20, encoding="utf-8")
        self.assert_refuses("license text bytes do not match manifest")

    def test_suspiciously_short_license_label_is_refused(self) -> None:
        self.release.license.write_text("CC0\n", encoding="utf-8")
        self.release.payload["licenses"][0].update(record(self.release.license))
        self.release.write_manifest()
        self.assert_refuses("short labels are not included license texts")

    def test_disallowed_license_is_refused_before_files_are_admitted(self) -> None:
        self.release.payload["licenses"][0]["id"] = "LicenseRef-Proprietary"
        self.release.payload["assets"][0]["license"] = "LicenseRef-Proprietary"
        self.release.write_manifest()
        self.assert_refuses("outside the reviewed release allowlist")

    def test_path_escape_is_refused(self) -> None:
        self.release.payload["assets"][0]["path"] = "../outside.svg"
        self.release.write_manifest()
        self.assert_refuses("must stay beneath the release root")

    def test_reserved_windows_path_is_refused(self) -> None:
        self.release.payload["assets"][0]["path"] = "assets/ui/NUL.svg"
        self.release.write_manifest()
        self.assert_refuses("reserved Windows filename")

    def test_escaped_lone_surrogate_is_a_controlled_refusal(self) -> None:
        self.release.payload["assets"][0]["path"] = "assets/ui/\ud800.svg"
        self.release.write_manifest()
        self.assert_refuses("invalid control or surrogate character")

    def test_non_https_provenance_is_refused(self) -> None:
        self.release.payload["assets"][0]["source_url"] = "file:///tmp/cursor.svg"
        self.release.write_manifest()
        self.assert_refuses("must be an HTTPS URL")

    def test_malformed_https_authority_is_a_controlled_refusal(self) -> None:
        self.release.payload["assets"][0]["source_url"] = "https://[broken/source"
        self.release.write_manifest()
        self.assert_refuses("not a valid HTTPS URL")

    def test_malformed_https_port_is_a_controlled_refusal(self) -> None:
        self.release.payload["assets"][0]["source_url"] = "https://example.invalid:bogus/source"
        self.release.write_manifest()
        self.assert_refuses("not a valid HTTPS URL")

    def test_executable_bytes_cannot_be_laundered_as_art(self) -> None:
        self.release.asset.write_bytes(b"MZsynthetic renamed executable")
        self.release.payload["assets"][0].update(record(self.release.asset))
        self.release.write_manifest()
        self.assert_refuses("forbidden Windows PE executable")

    def test_overlapping_asset_roots_are_refused(self) -> None:
        self.release.payload["asset_roots"] = ["assets", "assets/ui"]
        self.release.write_manifest()
        self.assert_refuses("asset roots overlap")

    def test_case_variant_overlapping_asset_roots_are_refused(self) -> None:
        self.release.payload["asset_roots"] = ["assets", "ASSETS/ui"]
        self.release.write_manifest()
        self.assert_refuses("asset roots overlap")

    def test_case_variant_license_path_is_not_outside_asset_root(self) -> None:
        self.release.payload["licenses"][0]["path"] = "ASSETS/ui/cursor.svg"
        self.release.write_manifest()
        self.assert_refuses("license text must be outside asset roots")

    def test_empty_declared_asset_root_is_refused(self) -> None:
        (self.root / "empty-assets").mkdir()
        self.release.payload["asset_roots"].append("empty-assets")
        self.release.write_manifest()
        self.assert_refuses("asset root contains no regular files")

    def test_asset_symlink_is_refused_when_supported(self) -> None:
        target = self.root / "outside.svg"
        target.write_text("synthetic outside\n", encoding="utf-8")
        self.release.asset.unlink()
        try:
            self.release.asset.symlink_to(target)
        except (NotImplementedError, OSError):
            self.skipTest("host cannot create symlinks")
        self.assert_refuses("symbolic link")

    def test_manifest_symlink_is_refused_when_supported(self) -> None:
        link = self.root / "manifest-link.json"
        try:
            link.symlink_to(self.release.manifest.name)
        except (NotImplementedError, OSError):
            self.skipTest("host cannot create symlinks")
        with self.assertRaisesRegex(audit.AuditError, "symbolic link"):
            audit.audit_release(self.root, link)

    def test_executable_mode_asset_is_refused_when_supported(self) -> None:
        self.release.asset.chmod(0o755)
        if not self.release.asset.stat().st_mode & 0o111:
            self.skipTest("host does not expose executable mode bits")
        self.assert_refuses("executable mode bits")

    def test_casefold_collision_is_refused_on_case_sensitive_hosts(self) -> None:
        second = self.root / "assets/ui/CURSOR.svg"
        second.write_text("synthetic second case\n", encoding="utf-8")
        if second.samefile(self.release.asset):
            self.skipTest("host filesystem is case-insensitive")
        second_entry = dict(self.release.payload["assets"][0])
        second_entry["path"] = "assets/ui/CURSOR.svg"
        second_entry.update(record(second))
        self.release.payload["assets"].append(second_entry)
        self.release.write_manifest()
        self.assert_refuses("case-insensitive collision")

    def test_casefold_directory_collision_is_refused_on_case_sensitive_hosts(self) -> None:
        upper = self.root / "assets/UI"
        if upper.exists():
            self.skipTest("host filesystem is case-insensitive")
        upper.mkdir()
        second = upper / "other.svg"
        second.write_text("synthetic directory collision\n", encoding="utf-8")
        second_entry = dict(self.release.payload["assets"][0])
        second_entry["path"] = "assets/UI/other.svg"
        second_entry.update(record(second))
        self.release.payload["assets"].append(second_entry)
        self.release.write_manifest()
        self.assert_refuses("case-insensitive sibling collision")


if __name__ == "__main__":
    unittest.main()
