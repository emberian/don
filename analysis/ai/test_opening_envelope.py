#!/usr/bin/env python3

import importlib.util
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("opening_envelope", HERE / "opening_envelope.py")
assert SPEC and SPEC.loader
opening_envelope = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = opening_envelope
SPEC.loader.exec_module(opening_envelope)


TRACE = """don.ai.accepted-production-trace.v1
meta\tminutes\t12\tseeds\t1\tfps\t15
policy\tseed\tseat\tframe\tkind\ttype\tcount
Ai\t0x5eed0001\t0\t0\tbuilding\tWoodcutter's Camp\t1
Ai\t0x5eed0001\t0\t15\tbuilding\tFarm\t1
Ai\t0x5eed0001\t0\t60\tunit\tCitizen\t1
Ai\t0x5eed0001\t0\t300\tbuilding\tMarket\t1
Ai\t0x5eed0001\t1\t1\tbuilding\tWoodcutter's Camp\t1
Ai\t0x5eed0001\t1\t16\tunit\tCitizen\t1
ShippedOpening\t0x5eed0001\t0\t0\tbuilding\tWoodcutter's Camp\t1
ShippedOpening\t0x5eed0001\t0\t75\tunit\tCitizen\t1
ShippedOpening\t0x5eed0001\t1\t1\tbuilding\tWoodcutter's Camp\t1
"""


class OpeningEnvelopeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.derived, cls.corpus = opening_envelope.load_inputs()

    def test_replay_corpus_contains_no_retail_ai_production_decisions(self):
        _rows, audit = opening_envelope.human_rows(
            self.derived, self.corpus, 12 * 60 * opening_envelope.FPS
        )
        self.assertGreater(audit["nonhuman_slots"], 0)
        self.assertEqual(audit["nonhuman_production_rows"], 0)
        self.assertEqual(audit["nonhuman_production_decisions"], 0)

    def test_trace_schema_rejects_unlabelled_input(self):
        with self.assertRaisesRegex(ValueError, "v1"):
            opening_envelope.parse_trace("policy,frame\nAi,0\n")

    def test_a_missing_seed_seat_run_fails_closed(self):
        incomplete = TRACE.rsplit("ShippedOpening", 1)[0]
        with self.assertRaisesRegex(ValueError, "ShippedOpening runs"):
            opening_envelope.build_report(
                self.derived,
                self.corpus,
                opening_envelope.parse_trace(incomplete),
            )

    def test_decision_weight_selects_the_missing_knowledge_surface(self):
        report = opening_envelope.build_report(
            self.derived,
            self.corpus,
            opening_envelope.parse_trace(TRACE),
        )
        recommendation = report["next_model_correction"]
        self.assertEqual(recommendation["status"], "diagnostic_priority")
        self.assertEqual(recommendation["family"], "knowledge_economy")
        self.assertEqual(set(recommendation["missing_types"]), {"Scholar", "University"})
        self.assertGreater(recommendation["fraction_of_human_production"], 0.05)
        self.assertFalse(report["scope"]["score_or_elo"])

    def test_candidate_first_issue_is_kept_beside_not_substituted_for_human_bounds(self):
        report = opening_envelope.build_report(
            self.derived,
            self.corpus,
            opening_envelope.parse_trace(TRACE),
        )
        farm = report["policy_traces"]["Ai"]["first_issue_trace"]["Farm"]
        self.assertEqual(farm["candidate_p50"], 15)
        self.assertIn("p50", farm["human_first_issue"])
        self.assertNotEqual(farm["candidate_p50"], farm["human_first_issue"]["p50"])


if __name__ == "__main__":
    unittest.main()
