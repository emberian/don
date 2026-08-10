#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline regressions for compact retail-controller release evidence."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SPEC = importlib.util.spec_from_file_location(
    "retail_control_evidence", HERE / "retail_control_evidence.py"
)
assert SPEC and SPEC.loader
evidence = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = evidence
SPEC.loader.exec_module(evidence)


class RetailControlEvidenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="don-retail-control-proof-")
        self.root = Path(self.temporary.name).resolve()
        for relative in (
            evidence.SOURCE_PATH,
            evidence.INCIDENT_PATH,
            evidence.LIFECYCLE_PATH,
            evidence.CLOSURE_PATH,
        ):
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, destination)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def load(self, relative: str) -> dict:
        return json.loads((self.root / relative).read_text(encoding="utf-8"))

    def write(self, relative: str, value: dict) -> None:
        (self.root / relative).write_text(
            json.dumps(value, indent=2, sort_keys=False) + "\n", encoding="utf-8"
        )

    def test_current_compact_evidence_is_source_bound_and_green(self) -> None:
        report = evidence.verify(
            self.root,
            self.root / evidence.LIFECYCLE_PATH,
            self.root / evidence.CLOSURE_PATH,
        )
        self.assertTrue(report["ok"])
        self.assertEqual(report["lifecycle"]["consecutive_stop_rearm_cycles"], 5)
        self.assertEqual(
            report["lifecycle"]["restored_bytes_after_every_stop"], "E8 45 67 3C 00"
        )
        self.assertTrue(report["incident_closure"]["candidate_bound_reversibility_soak"])
        self.assertEqual(
            report["incident_closure"]["original_incident_causality"],
            "unresolved-no-retained-stack",
        )

    def test_cycle_count_cannot_be_inflated(self) -> None:
        value = self.load(evidence.LIFECYCLE_PATH)
        value["result"]["consecutive_stop_rearm_cycles"] = 6
        self.write(evidence.LIFECYCLE_PATH, value)
        with self.assertRaisesRegex(evidence.EvidenceError, "cycle count drift"):
            evidence.verify_lifecycle(self.root, self.root / evidence.LIFECYCLE_PATH)

    def test_integer_cannot_stand_in_for_true(self) -> None:
        value = self.load(evidence.LIFECYCLE_PATH)
        value["result"]["final_park_acknowledged"] = 1
        self.write(evidence.LIFECYCLE_PATH, value)
        with self.assertRaisesRegex(evidence.EvidenceError, "final park acknowledgement drift"):
            evidence.verify_lifecycle(self.root, self.root / evidence.LIFECYCLE_PATH)

    def test_restored_bytes_are_exact_and_case_sensitive(self) -> None:
        value = self.load(evidence.LIFECYCLE_PATH)
        value["result"]["external_stop_read"]["bytes_hex"] = "e8 45 67 3c 00"
        self.write(evidence.LIFECYCLE_PATH, value)
        with self.assertRaisesRegex(evidence.EvidenceError, "restored call bytes drift"):
            evidence.verify_lifecycle(self.root, self.root / evidence.LIFECYCLE_PATH)

    def test_lifecycle_unknown_field_is_refused(self) -> None:
        value = self.load(evidence.LIFECYCLE_PATH)
        value["result"]["per_cycle_timestamps"] = []
        self.write(evidence.LIFECYCLE_PATH, value)
        with self.assertRaisesRegex(evidence.EvidenceError, "lifecycle result fields are invalid"):
            evidence.verify_lifecycle(self.root, self.root / evidence.LIFECYCLE_PATH)

    def test_source_prose_hash_drift_is_refused(self) -> None:
        path = self.root / evidence.SOURCE_PATH
        path.write_text(path.read_text(encoding="utf-8") + "\nsynthetic drift\n", encoding="utf-8")
        lifecycle = self.load(evidence.LIFECYCLE_PATH)
        lifecycle["evidence_source"]["sha256"] = evidence._sha256(path)
        self.write(evidence.LIFECYCLE_PATH, lifecycle)
        with self.assertRaisesRegex(evidence.EvidenceError, "supported lifecycle prose SHA-256"):
            evidence.verify_lifecycle(self.root, self.root / evidence.LIFECYCLE_PATH)

    def test_closure_cannot_claim_resolved_causality(self) -> None:
        value = self.load(evidence.CLOSURE_PATH)
        value["closure"]["original_incident_causality_remains_unresolved"] = False
        self.write(evidence.CLOSURE_PATH, value)
        with self.assertRaisesRegex(evidence.EvidenceError, "causality_remains_unresolved drift"):
            evidence.verify_closure(self.root, self.root / evidence.CLOSURE_PATH)

    def test_closure_is_bound_to_exact_lifecycle_artifact(self) -> None:
        value = self.load(evidence.CLOSURE_PATH)
        value["later_active_lifecycle"]["sha256"] = "0" * 64
        self.write(evidence.CLOSURE_PATH, value)
        with self.assertRaisesRegex(evidence.EvidenceError, "later lifecycle SHA-256 drift"):
            evidence.verify_closure(self.root, self.root / evidence.CLOSURE_PATH)

    def test_closure_cannot_turn_absent_dump_into_retained_dump(self) -> None:
        value = self.load(evidence.CLOSURE_PATH)
        value["original_incident"]["dump_retained"] = True
        self.write(evidence.CLOSURE_PATH, value)
        with self.assertRaisesRegex(evidence.EvidenceError, "incident dump_retained drift"):
            evidence.verify_closure(self.root, self.root / evidence.CLOSURE_PATH)

    def test_closure_does_not_accept_a_different_candidate_hash(self) -> None:
        value = self.load(evidence.CLOSURE_PATH)
        value["later_active_lifecycle"]["controller_dll_sha256"] = "1" * 64
        self.write(evidence.CLOSURE_PATH, value)
        with self.assertRaisesRegex(evidence.EvidenceError, "later controller DLL SHA-256 drift"):
            evidence.verify_closure(self.root, self.root / evidence.CLOSURE_PATH)


if __name__ == "__main__":
    unittest.main()
