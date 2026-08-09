#!/usr/bin/env python3
"""Synthetic regressions for exact component dependency provenance capture."""

from __future__ import annotations

import importlib.util
import io
import json
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "component_provenance", HERE / "component_provenance.py"
)
component_provenance = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = component_provenance
SPEC.loader.exec_module(component_provenance)


class SyntheticComponent:
    def __init__(self, root: Path) -> None:
        self.root = root
        (root / "payload").mkdir(parents=True)
        (root / "crates").mkdir()
        (root / "payload/component.wasm").write_bytes(b"\x00asm synthetic component\n")
        (root / "LICENSE").write_text("Synthetic first-party license text.\n", encoding="utf-8")
        (root / "Cargo.toml").write_text(
            """[workspace]
members = []

[workspace.package]
license = "GPL-3.0-or-later"

[package]
name = "component"
version = "1.2.3"
license.workspace = true
""",
            encoding="utf-8",
        )
        archive = root / "crates/dep-4.5.6.crate"
        self._write_archive(
            archive,
            {
                "dep-4.5.6/Cargo.toml.orig": (
                    b'[package]\nname = "dep"\nversion = "4.5.6"\nlicense = "MIT"\n'
                ),
                "dep-4.5.6/LICENSE-MIT": b"Synthetic dependency license text.\n",
            },
        )
        checksum = component_provenance.sha256(archive)
        (root / "Cargo.lock").write_text(
            f"""version = 4

[[package]]
name = "component"
version = "1.2.3"
dependencies = ["dep"]

[[package]]
name = "dep"
version = "4.5.6"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "{checksum}"
""",
            encoding="utf-8",
        )
        self.output = root / "release/component.json"
        self.evidence = root / "release/notices"

    @staticmethod
    def _write_archive(path: Path, files: dict[str, bytes]) -> None:
        with tarfile.open(path, "w:gz") as archive:
            for name, data in files.items():
                member = tarfile.TarInfo(name)
                member.size = len(data)
                member.mtime = 0
                archive.addfile(member, io.BytesIO(data))

    def capture(self) -> dict:
        return component_provenance.capture(
            self.root,
            self.output,
            self.root / "crates",
            self.evidence,
            "synthetic-component",
            "1.2.3",
            "component",
            self.root / "payload/component.wasm",
            self.root / "Cargo.lock",
        )


class ComponentProvenanceTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="don-component-proof-")
        self.root = Path(self.temporary.name)
        self.fixture = SyntheticComponent(self.root)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_capture_binds_lock_graph_archive_and_license_evidence(self) -> None:
        artifact = self.fixture.capture()
        result = component_provenance.verify(self.root, self.fixture.output)
        self.assertEqual(result["packages"], 2)
        self.assertEqual(result["registry_packages"], 1)
        dependency = artifact["selection"]["packages"][1]
        self.assertEqual(dependency["license"]["declared_expression"], "MIT")
        self.assertEqual(
            dependency["archive"]["sha256"], dependency["identity"]["checksum"]
        )
        self.assertFalse(artifact["claims"]["notice_sufficiency_reviewed"])
        self.assertFalse(artifact["claims"]["whole_product_payload"])

    def test_payload_byte_drift_is_refused(self) -> None:
        self.fixture.capture()
        (self.root / "payload/component.wasm").write_bytes(b"different\n")
        with self.assertRaisesRegex(component_provenance.ProvenanceError, "bytes drift"):
            component_provenance.verify(self.root, self.fixture.output)

    def test_claim_cannot_be_promoted_without_review(self) -> None:
        artifact = self.fixture.capture()
        artifact["claims"]["notice_sufficiency_reviewed"] = True
        self.fixture.output.write_text(json.dumps(artifact), encoding="utf-8")
        with self.assertRaisesRegex(component_provenance.ProvenanceError, "overstate"):
            component_provenance.verify(self.root, self.fixture.output)

    def test_artifact_evidence_path_cannot_escape_repository(self) -> None:
        artifact = self.fixture.capture()
        artifact["payload"]["path"] = "../outside.wasm"
        self.fixture.output.write_text(json.dumps(artifact), encoding="utf-8")
        with self.assertRaisesRegex(component_provenance.ProvenanceError, "escapes"):
            component_provenance.verify(self.root, self.fixture.output)

    def test_registry_archive_must_match_the_lock_checksum(self) -> None:
        archive = self.root / "crates/dep-4.5.6.crate"
        archive.write_bytes(archive.read_bytes() + b"tamper")
        with self.assertRaisesRegex(component_provenance.ProvenanceError, "checksum mismatch"):
            self.fixture.capture()

    def test_extracted_declaration_is_rederived(self) -> None:
        artifact = self.fixture.capture()
        declaration = self.root / artifact["selection"]["packages"][1]["license"][
            "declaration_evidence"
        ]["path"]
        declaration.write_text(
            '[package]\nname = "dep"\nversion = "4.5.6"\nlicense = "Apache-2.0"\n',
            encoding="utf-8",
        )
        record = artifact["selection"]["packages"][1]["license"]["declaration_evidence"]
        record.update(component_provenance.file_record(self.root, declaration))
        self.fixture.output.write_text(json.dumps(artifact), encoding="utf-8")
        with self.assertRaisesRegex(component_provenance.ProvenanceError, "declaration drift"):
            component_provenance.verify(self.root, self.fixture.output)

    def test_archive_path_escape_is_refused(self) -> None:
        archive = self.root / "crates/dep-4.5.6.crate"
        self.fixture._write_archive(
            archive,
            {
                "../escape": b"bad\n",
                "dep-4.5.6/Cargo.toml.orig": (
                    b'[package]\nname = "dep"\nversion = "4.5.6"\nlicense = "MIT"\n'
                ),
                "dep-4.5.6/LICENSE": b"text\n",
            },
        )
        checksum = component_provenance.sha256(archive)
        lock = (self.root / "Cargo.lock").read_text(encoding="utf-8")
        lock = lock.rsplit('checksum = "', 1)[0] + f'checksum = "{checksum}"\n'
        (self.root / "Cargo.lock").write_text(lock, encoding="utf-8")
        with self.assertRaisesRegex(component_provenance.ProvenanceError, "unsafe"):
            self.fixture.capture()


if __name__ == "__main__":
    unittest.main()
