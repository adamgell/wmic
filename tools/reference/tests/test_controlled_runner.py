import base64
import copy
from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from run_controlled import compare_payload


class ControlledRunnerTests(unittest.TestCase):
    def payload(self, name):
        from run_controlled import EXPECTED, EXPECTED_FILES
        cases = []
        for role in ('reference', 'candidate'):
            for case_id in EXPECTED[name]:
                case = {'id': case_id, 'role': role}
                if name == 'console':
                    case.update(state='completed', exit_status=0, termination_status=None,
                                screen_utf16_base64='QQA=', width=80, cursor_x=0, cursor_y=1)
                else:
                    case['result'] = {'id': case_id, 'state': 'completed', 'exit_status': (0x80041017 if name=='scripts' and case_id in ('error.cmd','error.ps1') else 0),
                        'arguments_utf16_base64': 'QQA=', 'stdout_base64': 'QQ==', 'stderr_base64': ''}
                    if name in ('files', 'unicode', 'session-files'):
                        case['files'] = {filename: 'QQ==' for filename in EXPECTED_FILES[name][case_id]}
                cases.append(case)
        return {'cases': cases, 'cleanup': True, 'errors': []}

    def test_exact_console_and_script_evidence_matches(self):
        for name, count in [('console', 9), ('scripts', 17), ('files', 22)]:
            with self.subTest(name=name):
                self.assertEqual(compare_payload(name, self.payload(name), count), [])

    def test_missing_duplicate_substituted_or_unknown_role_cases_fail_closed(self):
        for mutation in ('missing', 'duplicate', 'substitute', 'unknown-role'):
            data = self.payload('scripts')
            if mutation == 'missing': data['cases'].pop()
            elif mutation == 'duplicate': data['cases'].append(copy.deepcopy(data['cases'][0]))
            elif mutation == 'substitute':
                for case in data['cases']:
                    if case['id'] == 'query.cmd': case['id'] = 'unplanned.cmd'
            else: data['cases'].append(dict(data['cases'][0], role='other'))
            with self.subTest(mutation=mutation):
                self.assertTrue(compare_payload('scripts', data, 17))

    def test_matching_timeouts_and_invalid_statuses_are_never_success(self):
        for name, count in [('console', 9), ('scripts', 17), ('files', 22)]:
            for status in (None, True, -1, 0x100000000):
                data = self.payload(name)
                for case in data['cases']:
                    result = case if name == 'console' else case['result']
                    result['exit_status'] = status
                    if status is None: result['state'] = 'timeout'
                with self.subTest(name=name, status=status):
                    self.assertTrue(compare_payload(name, data, count))

    def test_status_stream_file_and_console_screen_regressions_are_detected(self):
        for name, count in [('scripts', 17), ('files', 22), ('console', 9)]:
            data = self.payload(name)
            candidate = next(c for c in data['cases'] if c['role'] == 'candidate')
            if name == 'console': candidate['screen_utf16_base64'] = 'QgA='
            elif name == 'files': candidate['files'][next(iter(candidate['files']))] = 'Qg=='
            else: candidate['result']['exit_status'] = 0x80041017
            with self.subTest(name=name): self.assertTrue(compare_payload(name, data, count))

    def test_cleanup_errors_cannot_be_hidden_by_matching_outputs(self):
        for key, value in [('cleanup', False), ('errors', ['cleanup failed'])]:
            data = self.payload('console'); data[key] = value
            self.assertTrue(compare_payload('console', data, 9))

    def test_equally_missing_or_malformed_evidence_is_not_a_match(self):
        for name,count,field in [('console',9,'screen_utf16_base64'),('console',9,'width'),('scripts',17,'arguments_utf16_base64'),('scripts',17,'stdout_base64')]:
            data=self.payload(name)
            for case in data['cases']:
                result=case if name=='console' else case['result']
                del result[field]
            with self.subTest(name=name,field=field):
                self.assertTrue(compare_payload(name,data,count))
        data=self.payload('console')
        for case in data['cases']:case['screen_utf16_base64']='not base64'
        self.assertTrue(compare_payload('console',data,9))


    def test_both_roles_omitting_all_owned_files_never_pass(self):
        data=self.payload('files')
        for case in data['cases']:case['files']={}
        self.assertTrue(compare_payload('files',data,22))


class NegativeProbeTests(unittest.TestCase):
    def test_required_negative_probes_must_complete_with_valid_bytes_and_dword(self):
        import json
        root=Path(__file__).resolve().parents[3]
        payload=json.loads((root/'docs/research/evidence/2026-10-15/controlled-writes.json').read_text())
        template=copy.deepcopy(payload['cases'][0])
        template['id']='process-create-null-directory';template['result']['id']=template['id']
        payload['probes']=[dict(copy.deepcopy(template),role=role) for role in ('reference','candidate')]
        self.assertEqual(compare_payload('named-lifecycle',payload,11),[])
        for changes in [{'state':'timeout','exit_status':None},{'exit_status':True},{'arguments_utf16_base64':'not base64'},{'stdout_base64':'not base64'},{'id':'other'}]:
            data=copy.deepcopy(payload)
            for probe in data['probes']:probe['result'].update(changes)
            with self.subTest(changes=changes):self.assertTrue(compare_payload('named-lifecycle',data,11))


class ControlledCliTests(unittest.TestCase):
    def test_existing_output_directory_is_invalid_evidence_exit_two(self):
        import subprocess
        import tempfile
        import run_controlled
        with tempfile.TemporaryDirectory() as directory:
            result=subprocess.run([sys.executable,run_controlled.__file__,
                '--vm','unused','--target','aarch64-pc-windows-msvc',
                '--binary',str(Path(directory)/'missing.exe'),'--out',directory],capture_output=True,text=True)
            self.assertEqual(result.returncode,2,result.stdout+result.stderr)
            self.assertIn('fresh output directory',result.stderr)


class ScriptExecutionTests(unittest.TestCase):
    def test_matching_policy_blocked_scripts_are_not_compatibility_successes(self):
        data=ControlledRunnerTests().payload('scripts')
        for case in data['cases']:
            if case['id']=='query.ps1':
                case['result'].update(exit_status=1,stdout_base64='',stderr_base64='c2NyaXB0cyBkaXNhYmxlZA==')
        self.assertTrue(compare_payload('scripts',data,17))
