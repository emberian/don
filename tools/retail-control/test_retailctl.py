import importlib.util
import json
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location("retailctl", Path(__file__).with_name("retailctl.py"))
assert SPEC and SPEC.loader
retailctl = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(retailctl)


class RetailCtlTests(unittest.TestCase):
    def test_controller_generations_have_isolated_paths_and_unique_dll_names(self):
        self.assertEqual(
            retailctl.generation_root("v1"), r"C:\Users\Public\don-retail-control"
        )
        self.assertEqual(
            retailctl.generation_root("trajectory-2"),
            r"C:\Users\Public\don-retail-control-trajectory-2",
        )
        self.assertEqual(retailctl.generation_dll("v1"), "retail_control.dll")
        self.assertEqual(
            retailctl.generation_dll("trajectory-2"), "retail_control-trajectory-2.dll"
        )

    def test_unsafe_controller_generation_is_refused(self):
        for generation in ["", "../shared", "v2 & whoami", "has space", "x" * 49]:
            with self.assertRaises(SystemExit):
                retailctl.generation_root(generation)

    def test_command_tokens_accept_the_documented_protocol(self):
        for words in [
            ["observe"], ["pause", "1"], ["speed", "3"], ["speed-up"],
            ["checksum"], ["halt", "0", "12", "13"],
            ["move", "0", "100", "200", "2", "1", "-1", "-1", "0", "12"],
            ["attack", "0", "1", "22", "0", "2", "12", "13"],
            ["trace-move", "0", "12", "100", "200", "120"],
            ["observe-guys", "0", "12"],
        ]:
            retailctl.validate_words(words)

    def test_shell_metacharacters_are_refused(self):
        for words in [["observe&whoami"], ["pause", "1>pwn"], ["move", "$(x)"]]:
            with self.assertRaises(SystemExit):
                retailctl.validate_words(words)

    def test_unknown_verb_is_refused(self):
        with self.assertRaises(SystemExit):
            retailctl.validate_words(["cheat"])

    def test_live_trajectory_artifact_is_bounded_and_aslr_normalized(self):
        artifact = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-move-trajectory-v1.json")
            .read_text()
        )
        self.assertEqual(artifact["schema"], "don.retail-move-trajectory.v1")
        self.assertEqual(artifact["termination"], "trace-complete")
        self.assertEqual(artifact["terminal"]["pause"], 1)
        self.assertEqual(artifact["terminal"]["order"]["length"], 0)
        self.assertEqual(
            [sample["position"]["x"] for sample in artifact["samples"]],
            [2938, 2972, 3006, 3040, 3074, 3096],
        )
        self.assertTrue(all(
            sample["order"]["preferred_vtable"] == "0x00b4a12c"
            for sample in artifact["samples"][:-1]
        ))
        self.assertFalse(artifact["checksum"]["available"])


if __name__ == "__main__":
    unittest.main()
