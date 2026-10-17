"""Exercise the offline gate used to combine completed controlled runs."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import run_controlled
import test_controlled_runner


class ControlledAggregationTests(unittest.TestCase):
    def fixture(self, directory):
        root = Path(directory)
        binary = root / 'candidate.exe'
        binary.write_bytes(b'candidate bytes')
        sha = hashlib.sha256(binary.read_bytes()).hexdigest()
        payload = test_controlled_runner.ControlledRunnerTests().payload('console')
        payload['sha256'] = sha
        evidence = root / 'console.json'
        evidence.write_text(json.dumps(payload))
        script = Path(run_controlled.__file__).resolve().parents[2] / 'tools/windows/console-capture.ps1'
        report = {'artifact': {'target': 'aarch64-pc-windows-msvc', 'sha256': sha},
                  'requested_suites': ['console'], 'completed': True,
                  'tests': {'console': {'total': 9, 'differences': [],
                      'reference_architecture': 'arm64',
                      'script_sha256': hashlib.sha256(script.read_bytes()).hexdigest(),
                      'evidence_sha256': hashlib.sha256(evidence.read_bytes()).hexdigest()}}}
        return root, binary, report

    def invoke(self, root, binary, report):
        source = root / 'source-report.json'
        source.write_text(json.dumps(report))
        return subprocess.run([sys.executable, run_controlled.__file__,
            '--aggregate-report', str(source), '--target', 'aarch64-pc-windows-msvc',
            '--binary', str(binary), '--suite', 'console', '--out', str(root / 'out')],
            capture_output=True, text=True)

    def test_completed_current_evidence_is_aggregated_with_exact_provenance(self):
        with tempfile.TemporaryDirectory() as directory:
            root, binary, report = self.fixture(directory)
            result = self.invoke(root, binary, report)
            self.assertEqual(result.returncode, 0, result.stderr)
            aggregate = json.loads((root / 'out/report.json').read_text())
            self.assertIs(aggregate['completed'], True)
            self.assertIs(aggregate['tests']['console']['source_run_completed'], True)
            self.assertEqual((root / 'out/console.json').read_bytes(), (root / 'console.json').read_bytes())
            self.assertEqual(aggregate['tests']['console']['source_report_sha256'],
                             hashlib.sha256((root / 'source-report.json').read_bytes()).hexdigest())

    def test_incomplete_source_report_is_rejected_even_when_its_suite_matches(self):
        for completed in (False, None, 1):
            with self.subTest(completed=completed), tempfile.TemporaryDirectory() as directory:
                root, binary, report = self.fixture(directory)
                report['completed'] = completed
                result = self.invoke(root, binary, report)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn('completed source report', result.stderr)
                self.assertFalse((root / 'out').exists())

    def test_candidate_harness_evidence_hash_and_inventory_must_validate(self):
        for mutation in ('candidate', 'target', 'harness', 'evidence', 'inventory', 'differences'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root, binary, report = self.fixture(directory)
                entry = report['tests']['console']
                if mutation == 'candidate': report['artifact']['sha256'] = '0' * 64
                elif mutation == 'target': report['artifact']['target'] = 'i686-pc-windows-msvc'
                elif mutation == 'harness': entry['script_sha256'] = '0' * 64
                elif mutation == 'evidence': entry['evidence_sha256'] = '0' * 64
                elif mutation == 'inventory': report['requested_suites'].append('scripts')
                else: entry['differences'] = ['screen mismatch']
                result = self.invoke(root, binary, report)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertFalse((root / 'out').exists())

    def test_hash_valid_but_failed_fixture_cleanup_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root, binary, report = self.fixture(directory)
            evidence = root / 'console.json'
            payload = json.loads(evidence.read_text())
            payload['cleanup'] = False
            evidence.write_text(json.dumps(payload))
            report['tests']['console']['evidence_sha256'] = hashlib.sha256(evidence.read_bytes()).hexdigest()
            result = self.invoke(root, binary, report)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertFalse((root / 'out').exists())

    def test_unselected_suite_does_not_replace_or_invalidate_current_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root, binary, report = self.fixture(directory)
            report['requested_suites'].append('scripts')
            report['tests']['scripts'] = {'total': 17, 'differences': [],
                'reference_architecture': 'arm64', 'script_sha256': '0' * 64,
                'evidence_sha256': '0' * 64}
            result = self.invoke(root, binary, report)
            self.assertEqual(result.returncode, 0, result.stderr)
            aggregate = json.loads((root / 'out/report.json').read_text())
            self.assertEqual(set(aggregate['tests']), {'console'})


if __name__ == '__main__':
    unittest.main()
