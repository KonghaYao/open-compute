"""Fail-closed checks for release qualification inputs (no installed services)."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

from prepare import digest, previous_release, verify_migrations, verify_release
from run import Qualification


class QualificationInputs(unittest.TestCase):
    def test_stop_waits_for_manager_and_descendants_and_enforces_deadline(self):
        qualification = object.__new__(Qualification)
        tree = SimpleNamespace(stdout="41 1\n42 41\n", returncode=0)
        alive = SimpleNamespace(stdout="42\n", returncode=0)
        gone = SimpleNamespace(stdout="", returncode=1)
        with patch.object(qualification, "command", side_effect=[tree, gone, alive, gone, gone]), \
             patch.object(qualification, "pid", side_effect=[41, 41, 0]), \
             patch.object(qualification, "ocd") as stop, \
             patch("run.time.sleep"):
            qualification.stop()
            stop.assert_called_once_with("stop")
        with patch.object(qualification, "command", return_value=tree), \
             patch.object(qualification, "pid", return_value=41), \
             patch.object(qualification, "ocd"), \
             patch("run.time.monotonic", side_effect=[0, 31]):
            with self.assertRaisesRegex(AssertionError, "did not disappear"):
                qualification.stop()

    def test_previous_is_latest_lower_official_stable(self):
        releases = [{"tagName": tag, "isDraft": draft, "isPrerelease": pre}
                    for tag, draft, pre in [("v0.2.2", False, False), ("v0.2.3", False, False),
                                            ("v0.2.4", True, False), ("v0.2.4-rc.1", False, True)]]
        self.assertEqual(previous_release(releases, "v0.2.3"), "v0.2.2")
        with self.assertRaises(ValueError):
            previous_release(releases, "v0.2.0")

    def test_modified_artifact_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            filename = "ocd-v0.2.2-linux-arm64"
            binary = directory / filename
            binary.write_bytes(b"original")
            sha = digest(binary)
            (directory / "release.json").write_text(json.dumps({
                "schemaVersion": 1, "tag": "v0.2.2", "version": "0.2.2", "artifacts": [{
                    "target": "linux-arm64", "filename": filename, "bytes": 8, "sha256": sha}]}))
            (directory / "SHA256SUMS").write_text(
                f'{sha}  {filename}\n{digest(directory / "release.json")}  release.json\n')
            verify_release(directory, "v0.2.2", "linux-arm64")
            binary.write_bytes(b"modified")
            with self.assertRaises(AssertionError):
                verify_release(directory, "v0.2.2", "linux-arm64")

    def test_published_migration_edits_are_refused(self):
        name = "crates/storage/refinery-migrations/V1__init.sql"
        with patch("prepare.command", return_value=name), \
             patch("prepare.subprocess.check_output", return_value=b"published"), \
             patch("prepare.Path.read_bytes", return_value=b"edited"):
            with self.assertRaisesRegex(AssertionError, "published migration changed"):
                verify_migrations("v0.2.2")
