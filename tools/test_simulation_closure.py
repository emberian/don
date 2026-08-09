import importlib.util
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("closure", ROOT / "tools" / "simulation-closure.py")
closure = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(closure)


class ClosureTest(unittest.TestCase):
    def test_compiled_inventory_is_exhaustive_and_fail_closed(self):
        replay = json.loads((ROOT / "schema" / "replay-validation.json").read_text())
        report = closure.build(closure.static_rows(), replay)
        self.assertFalse(report["ready"])
        self.assertEqual(report["summary"]["tick"]["total"], 29)
        self.assertEqual(report["summary"]["orders"]["total"], 28)
        self.assertEqual(report["summary"]["group_actions"]["total"], 42)
        self.assertEqual(report["summary"]["opcodes"]["total"], 82)
        self.assertEqual(report["summary"]["checksums"]["total"], 15)
        rules = next(x for x in report["domains"]["checksums"] if x["name"] == "rules")
        world = next(x for x in report["domains"]["checksums"] if x["name"] == "world")
        self.assertTrue(rules["complete"])
        self.assertFalse(world["complete"])


if __name__ == "__main__":
    unittest.main()
