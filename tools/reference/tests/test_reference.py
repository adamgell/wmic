import base64
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))
import compare
import parallels


def sample_case():
    return {
        "id": "binary-case",
        "arguments": "os",
        "arguments_utf16_base64": "bwBzAA==",
        "command_line_utf16_base64": "dwBtAGkAYwAuAGUAeABlACAAbwBzAA==",
        "state": "completed",
        "exit_status": 0x80041017,
        "stdout_base64": "//5BAA0ACgA=",
        "stderr_base64": "RXJyb3INCg==",
    }


class ReferenceTests(unittest.TestCase):
    def test_roundtrip_preserves_binary_streams_and_unsigned_exit(self):
        with tempfile.TemporaryDirectory() as tmp:
            manifest = parallels.write_capture(
                {"schema_version": 1, "profile": {"role": "reference"}, "cases": [sample_case()]},
                Path(tmp),
            )
            case = manifest["cases"][0]
            self.assertEqual((Path(tmp) / case["stdout"]["file"]).read_bytes(), b"\xff\xfeA\x00\r\x00\n\x00")
            self.assertEqual((Path(tmp) / case["stderr"]["file"]).read_bytes(), b"Error\r\n")
            self.assertEqual(case["exit_status"], 0x80041017)
            self.assertEqual(case["command_line_utf16_base64"], sample_case()["command_line_utf16_base64"])

    def test_comparison_does_not_normalize_line_endings(self):
        expected = sample_case()
        actual = copy.deepcopy(expected)
        expected["stdout_base64"] = base64.b64encode(b"A\r\r\n").decode()
        actual["stdout_base64"] = base64.b64encode(b"A\r\n").decode()
        differences = compare.compare_cases(expected, actual)
        self.assertTrue(any("stdout" in d for d in differences))

    def test_timeout_is_not_a_successful_exit(self):
        expected = sample_case()
        expected["exit_status"] = 0
        actual = copy.deepcopy(expected)
        actual.update(state="timeout", exit_status=None)
        differences = compare.compare_cases(expected, actual)
        self.assertTrue(any("state" in d for d in differences))
        self.assertTrue(any("exit_status" in d for d in differences))

    def test_rawxml_timestamp_normalization_requires_explicit_reference_policy(self):
        expected = sample_case()
        actual = copy.deepcopy(expected)
        expected["stdout_base64"] = base64.b64encode(b'<COMMAND STARTTIME="10-15-2026T19:49:19"><VALUE>A</VALUE></COMMAND>').decode()
        actual["stdout_base64"] = base64.b64encode(b'<COMMAND STARTTIME="10-15-2026T20:02:02"><VALUE>A</VALUE></COMMAND>').decode()
        self.assertTrue(compare.compare_cases(expected, actual))
        expected["normalizations"] = [{"stream": "stdout", "kind": "wmic-command-starttime"}]
        self.assertEqual(compare.compare_cases(expected, actual), [])

    def test_timestamp_policy_does_not_hide_changes_in_property_values(self):
        expected = sample_case()
        actual = copy.deepcopy(expected)
        expected["normalizations"] = [{"stream": "stdout", "kind": "wmic-command-starttime"}]
        expected["stdout_base64"] = base64.b64encode(b'<COMMAND STARTTIME="a"><VALUE>STARTTIME="a"</VALUE></COMMAND>').decode()
        actual["stdout_base64"] = base64.b64encode(b'<COMMAND STARTTIME="b"><VALUE>STARTTIME="b"</VALUE></COMMAND>').decode()
        self.assertTrue(compare.compare_cases(expected, actual))

    def test_stderr_is_not_merged_with_stdout(self):
        expected = sample_case()
        actual = copy.deepcopy(expected)
        actual["stdout_base64"] = base64.b64encode(b"\xff\xfeA\x00\r\x00\n\x00Error\r\n").decode()
        actual["stderr_base64"] = ""
        differences = compare.compare_cases(expected, actual)
        self.assertTrue(any("stdout" in d for d in differences))
        self.assertTrue(any("stderr" in d for d in differences))

    def test_duplicate_ids_and_path_traversal_are_rejected_before_writes(self):
        for cases in [[sample_case(), sample_case()], [dict(sample_case(), id="../escape")]]:
            with self.subTest(cases=cases), tempfile.TemporaryDirectory() as tmp:
                with self.assertRaises(ValueError):
                    parallels.write_capture({"cases": cases}, Path(tmp))
                self.assertEqual(list(Path(tmp).iterdir()), [])

    def test_invalid_base64_does_not_create_partial_artifacts(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ValueError):
                parallels.write_capture({"cases": [dict(sample_case(), stdout_base64="%%%not-base64")]}, Path(tmp))
            self.assertEqual(list(Path(tmp).iterdir()), [])

    def test_refuses_persisting_password_arguments(self):
        manifest = {"cases": [{"id": "secret", "arguments": "/password:real-secret os get Caption", "operation": "read_only"}]}
        with self.assertRaises(ValueError):
            parallels.validate_manifest(manifest)

    def test_controlled_writes_are_not_part_of_default_capture(self):
        manifest = {"cases": [{"id": "write", "arguments": "process call create example", "operation": "controlled_write"}]}
        with self.assertRaises(ValueError):
            parallels.validate_manifest(manifest)

    def test_corrupt_stored_stream_is_not_accepted_as_reference(self):
        with tempfile.TemporaryDirectory() as tmp:
            manifest = parallels.write_capture({"cases": [sample_case()]}, Path(tmp))
            case = manifest["cases"][0]
            (Path(tmp) / case["stdout"]["file"]).write_bytes(b"changed")
            with self.assertRaises(ValueError):
                compare.compare_cases(case, sample_case(), Path(tmp), None)

    def test_compare_cli_exits_one_for_byte_difference_and_zero_for_equal(self):
        with tempfile.TemporaryDirectory() as tmp:
            expected = Path(tmp) / "expected.json"
            actual = Path(tmp) / "actual.json"
            payload = {"cases": [sample_case()]}
            expected.write_text(json.dumps(payload))
            actual.write_text(json.dumps(payload))
            cmd = [sys.executable, str(TOOLS / "compare.py"), str(expected), str(actual)]
            self.assertEqual(subprocess.run(cmd, capture_output=True).returncode, 0)
            payload["cases"][0]["stderr_base64"] = ""
            actual.write_text(json.dumps(payload))
            result = subprocess.run(cmd, capture_output=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn(b"stderr", result.stdout)



class TransportTests(unittest.TestCase):
    def test_known_transport_failure_can_replay_explicitly_safe_script(self):
        from unittest.mock import patch
        failed = subprocess.CompletedProcess([], 1, b'', b'PrlJob_GetRetCode: Invalid argument.')
        success = subprocess.CompletedProcess([], 0, b'exact bytes\r\n', b'')
        with patch.object(parallels.subprocess, 'run', side_effect=[failed,success]) as run:
            self.assertEqual(parallels.run_guest_script('selected-vm','read-only script',replay_safe=True), b'exact bytes\r\n')
            self.assertEqual(run.call_count,2)

    def test_script_is_not_replayed_without_explicit_safety_flag(self):
        from unittest.mock import patch
        failed = subprocess.CompletedProcess([], 1, b'', b'PrlJob_GetResult: Invalid argument.')
        with patch.object(parallels.subprocess, 'run', return_value=failed) as run:
            with self.assertRaises(RuntimeError): parallels.run_guest_script('selected-vm','mutation script')
            self.assertEqual(run.call_count,1)

    def test_guest_failure_is_not_replayed(self):
        from unittest.mock import patch
        failed = subprocess.CompletedProcess([], 1, b'', b'Application failed')
        with patch.object(parallels.subprocess, 'run', return_value=failed) as run:
            with self.assertRaises(RuntimeError): parallels.run_guest_script('selected-vm','read-only script',replay_safe=True)
            self.assertEqual(run.call_count,1)


if __name__ == "__main__":
    unittest.main()
