#!/usr/bin/env python3

from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("owned_bootstrap", HERE / "bootstrap.py")
assert SPEC and SPEC.loader
bootstrap = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bootstrap
SPEC.loader.exec_module(bootstrap)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class Fixture:
    def __init__(self, root: Path):
        self.retail = root / "Retail"
        self.workspace = root / "don"
        self.manifest = root / "manifest.json"
        (self.retail / "DATA").mkdir(parents=True)
        (self.retail / "ai" / "SCRIPTS").mkdir(parents=True)
        self.workspace.mkdir()
        (self.workspace / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        (self.workspace / "GOAL.md").write_text("goal\n", encoding="utf-8")
        (self.workspace / ".gitignore").write_text("ron-data/\n", encoding="utf-8")
        self.exe = b"supported-exe"
        self.rules = b"<RULES/>\n"
        self.ai = b"int ai economic() {}\n"
        (self.retail / "RiseOfNations.EXE").write_bytes(self.exe)
        (self.retail / "DATA" / "RULES.XML").write_bytes(self.rules)
        (self.retail / "ai" / "SCRIPTS" / "ECONOMIC.BHS").write_bytes(self.ai)
        payload = {
            "schema": "don.owned-inputs.v1",
            "retail_identity": {
                "source": "riseofnations.exe", "size": len(self.exe), "sha256": digest(self.exe),
            },
            "files": [
                {
                    "source": "Data/rules.xml", "destination": "rules.xml",
                    "size": len(self.rules), "sha256": digest(self.rules),
                },
                {
                    "source": "AI/scripts/economic.bhs",
                    "destination": "ai-scripts/economic.bhs",
                    "size": len(self.ai), "sha256": digest(self.ai),
                },
            ],
        }
        self.manifest.write_text(json.dumps(payload), encoding="utf-8")

    def load(self):
        return bootstrap.load_manifest(self.manifest)


class BootstrapTests(unittest.TestCase):
    def test_casefold_source_resolution_and_create_only_install_are_idempotent(self):
        with tempfile.TemporaryDirectory() as temp:
            fixture = Fixture(Path(temp))
            first = bootstrap.install(fixture.retail, fixture.workspace, fixture.load(), dry_run=False)
            self.assertEqual((first["installed"], first["already_present"]), (2, 0))
            self.assertEqual((fixture.workspace / "ron-data" / "rules.xml").read_bytes(), fixture.rules)
            self.assertEqual(
                (fixture.workspace / "ron-data" / "ai-scripts" / "economic.bhs").read_bytes(),
                fixture.ai,
            )
            second = bootstrap.install(fixture.retail, fixture.workspace, fixture.load(), dry_run=False)
            self.assertEqual((second["installed"], second["already_present"]), (0, 2))

    def test_every_source_is_verified_before_dry_run_or_write(self):
        with tempfile.TemporaryDirectory() as temp:
            fixture = Fixture(Path(temp))
            (fixture.retail / "DATA" / "RULES.XML").write_bytes(b"wrong")
            with self.assertRaisesRegex(bootstrap.BootstrapError, "unsupported size|SHA-256"):
                bootstrap.install(fixture.retail, fixture.workspace, fixture.load(), dry_run=False)
            self.assertFalse((fixture.workspace / "ron-data").exists())

    def test_dry_run_is_non_mutating(self):
        with tempfile.TemporaryDirectory() as temp:
            fixture = Fixture(Path(temp))
            result = bootstrap.install(fixture.retail, fixture.workspace, fixture.load(), dry_run=True)
            self.assertEqual(result["to_install"], 2)
            self.assertEqual(result["installed"], 0)
            self.assertFalse((fixture.workspace / "ron-data").exists())

    def test_existing_mismatch_is_refused_without_overwrite(self):
        with tempfile.TemporaryDirectory() as temp:
            fixture = Fixture(Path(temp))
            output = fixture.workspace / "ron-data"
            output.mkdir()
            destination = output / "rules.xml"
            destination.write_bytes(b"user-data")
            with self.assertRaisesRegex(bootstrap.BootstrapError, "existing destination"):
                bootstrap.install(fixture.retail, fixture.workspace, fixture.load(), dry_run=False)
            self.assertEqual(destination.read_bytes(), b"user-data")
            self.assertFalse((output / "ai-scripts" / "economic.bhs").exists())

    def test_symlink_source_and_destination_are_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            fixture = Fixture(Path(temp))
            real = fixture.retail / "DATA" / "RULES.XML"
            real.rename(fixture.retail / "DATA" / "REAL.XML")
            real.symlink_to(fixture.retail / "DATA" / "REAL.XML")
            with self.assertRaisesRegex(bootstrap.BootstrapError, "symbolic link"):
                bootstrap.verify_sources(fixture.retail, fixture.load())

        with tempfile.TemporaryDirectory() as temp:
            fixture = Fixture(Path(temp))
            output = fixture.workspace / "ron-data"
            elsewhere = Path(temp) / "elsewhere"
            output.mkdir()
            elsewhere.mkdir()
            (output / "ai-scripts").symlink_to(elsewhere, target_is_directory=True)
            with self.assertRaisesRegex(bootstrap.BootstrapError, "symbolic link"):
                bootstrap.install(fixture.retail, fixture.workspace, fixture.load(), dry_run=False)

    def test_manifest_rejects_escape_and_case_collisions(self):
        with tempfile.TemporaryDirectory() as temp:
            fixture = Fixture(Path(temp))
            payload = json.loads(fixture.manifest.read_text(encoding="utf-8"))
            payload["files"][0]["destination"] = "../rules.xml"
            fixture.manifest.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(bootstrap.BootstrapError, "escapes"):
                fixture.load()


if __name__ == "__main__":
    unittest.main()
