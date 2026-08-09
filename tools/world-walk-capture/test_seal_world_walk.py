#!/usr/bin/env python3

import hashlib
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
import zlib

import seal_world_walk as seal


ROOT = Path(__file__).resolve().parents[2]
EXE = ROOT / "ron-bin" / "riseofnations.exe"


def checkpoint(image: bytes) -> dict:
    world = zlib.adler32(image, 1) & 0xFFFFFFFF
    channels = [1] * 16
    channels[11] = world
    channels[15] = sum(channels[:15]) & 0xFFFFFFFF
    words = [f"0x{word:08x}" for word in channels]
    return {
        "schema": seal.CHECKPOINT_SCHEMA,
        "replay": {
            "name": "Playback.rcx",
            "bytes": 10,
            "sha256": "1" * 64,
            "payload_bytes": 20,
            "payload_sha256": "2" * 64,
            "version": "00.2024.06.20",
        },
        "join": {
            "key": "CommandPackage::group",
            "group": 2,
            "reporters": 2,
            "all_16_channels_identical": True,
        },
        "world_checksum": f"0x{world:08x}",
        "peers": [
            {
                "play": play,
                "stamp": 12 + play,
                "packet_bytes": 65,
                "packet_sha256": "3" * 64,
                "packet_evidence_sha256": str(4 + play) * 64,
                "channels": words,
            }
            for play in (0, 1)
        ],
        "byte_agreement_claimed": False,
    }


def context(image: bytes) -> dict:
    world = zlib.adler32(image, 1) & 0xFFFFFFFF
    return {
        "schema": seal.CONTEXT_SCHEMA,
        "controller": {
            "generation": "gen-7-world-capture",
            "dll_sha256": "6" * 64,
            "attempt": 1,
            "epoch": 1,
            "ready_sha256": "7" * 64,
            "loaded_module_manifest_sha256": "8" * 64,
        },
        "process": {
            "pid": 42,
            "creation_time_100ns": "123456789",
            "image_base": "0x00400000",
            "main_thread_id": 77,
            "retail_executable_sha256": seal.EXPECTED_EXE_SHA256,
        },
        "hook": {
            "callsite_va": f"0x{seal.WORLD_CALLSITE_VA:08x}",
            "original_bytes": seal.WORLD_CALLSITE_BYTES.hex(),
            "world_walker_va": f"0x{seal.WORLD_WALK_VA:08x}",
            "world_walker_bytes": seal.WORLD_WALK_BYTES,
            "world_walker_sha256": seal.WORLD_WALK_SHA256,
            "restoration_owner": "retail-controller-lifecycle",
            "restored_original": True,
            "adapter_active_at_copy": False,
        },
        "capture": {
            "sequence": 1,
            "phase": "frozen",
            "fault": "none",
            "group": 2,
            "target_checksum": f"0x{world:08x}",
            "original_checksum": f"0x{world:08x}",
            "original_bytes": len(image),
            "captured_checksum": f"0x{world:08x}",
            "captured_bytes": len(image),
            "walk_calls": 3,
            "tag_calls": 1,
            "thread_id": 77,
            "image_sha256": hashlib.sha256(image).hexdigest(),
            "inflight_at_copy": 0,
            "replay_sha256": "1" * 64,
            "replay_payload_sha256": "2" * 64,
        },
    }


class WorldWalkSealerTests(unittest.TestCase):
    def test_supported_executable_binds_complete_walker_and_callsite(self):
        identity = seal.pe_identity(EXE.read_bytes())
        self.assertEqual(identity["sha256"], seal.EXPECTED_EXE_SHA256)
        self.assertEqual(identity["world_walker"]["bytes"], 903)
        self.assertEqual(identity["world_walker"]["sha256"], seal.WORLD_WALK_SHA256)
        self.assertEqual(identity["world_callsite"]["bytes"], "e8e8f2d7ff")

    def test_checksum_report_alone_has_no_localization(self):
        image = b"retail-world-image"
        parsed = seal.parse_checkpoint(json.dumps(checkpoint(image)).encode())
        self.assertEqual(parsed["join"]["group"], 2)
        localization = seal.localize(image, None, None)
        self.assertFalse(localization["available"])
        self.assertIsNone(localization["first_difference"])

    def test_sealed_artifact_is_content_addressed_and_read_only(self):
        image = b"retail-world-image"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image_path = root / "image.bin"
            checkpoint_path = root / "checkpoint.json"
            context_path = root / "context.json"
            output = root / "sealed"
            image_path.write_bytes(image)
            checkpoint_path.write_text(json.dumps(checkpoint(image)))
            context_path.write_text(json.dumps(context(image)))
            destination = seal.seal(SimpleNamespace(
                image=image_path,
                checkpoint=checkpoint_path,
                context=context_path,
                executable=EXE,
                output=output,
                model_image=None,
                section_map=None,
            ))
            manifest = json.loads((destination / "manifest.json").read_text())
            self.assertEqual(destination.name, manifest["artifact_id"])
            self.assertEqual(manifest["image"]["sha256"], hashlib.sha256(image).hexdigest())
            self.assertFalse(manifest["localization"]["available"])
            self.assertFalse(manifest["simulation_agreement_claimed"])
            self.assertEqual((destination / "image.bin").stat().st_mode & 0o222, 0)

    def test_peer_disagreement_is_refused_before_image_admission(self):
        image = b"retail-world-image"
        report = checkpoint(image)
        report["peers"][1]["channels"][0] = "0x00000002"
        with self.assertRaisesRegex(seal.Refusal, "total|disagreement"):
            seal.parse_checkpoint(json.dumps(report).encode())


if __name__ == "__main__":
    unittest.main()
