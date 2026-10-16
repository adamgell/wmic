import base64, copy, json, subprocess, sys, tempfile, unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import run_corpus

class CorpusGateTests(unittest.TestCase):
    def case(self):
        return {'id':'stable','arguments':'os get Caption','arguments_utf16_base64':base64.b64encode('os get Caption'.encode('utf-16le')).decode(),'state':'completed','exit_status':0,'stdout_base64':base64.b64encode(b'Caption\r\r\nWindows\r\r\n').decode(),'stderr_base64':''}
    def fixture(self):
        profile={'computer':'TEST','identity':'TEST\\user','culture':'en-US','ui_culture':'en-US','ansi_codepage':1252,'oem_codepage':437,'console_output_codepage':437,'directory':'C:\\','launcher_architecture':'ARM64','launch_api':'CreateProcessW','output_mode':'native pipes'}
        return {'schema_version':1,'profile':profile,'cases':[self.case()]}
    def test_missing_duplicate_and_unexpected_cases_fail_instead_of_inflating_matches(self):
        manifest={'cases':[{'id':'stable','arguments':'os get Caption'}]}
        expected=self.fixture()
        for cases in [[],[self.case(),self.case()],[dict(self.case(),id='extra')]]:
            actual=self.fixture();actual['cases']=cases
            with self.subTest(cases=cases),self.assertRaises(ValueError):run_corpus.compare_suite(manifest,expected,actual)
    def test_exact_bytes_and_full_status_are_required_and_timeouts_never_count_as_pass(self):
        expected=self.fixture();manifest={'cases':[{'id':'stable','arguments':'os get Caption'}]}
        self.assertEqual(run_corpus.compare_suite(manifest,expected,copy.deepcopy(expected))['passed'],['stable'])
        for changes in [{'stdout_base64':base64.b64encode(b'Caption\r\nWindows\r\n').decode()},{'exit_status':0x80041017},{'state':'timeout','exit_status':None}]:
            actual=copy.deepcopy(expected);actual['cases'][0].update(changes)
            with self.subTest(changes=changes):self.assertTrue(run_corpus.compare_suite(manifest,expected,actual)['differences'])
        expected['cases'][0].update(state='timeout',exit_status=None)
        with self.assertRaises(ValueError):run_corpus.compare_suite(manifest,expected,copy.deepcopy(expected))
    def test_changed_profile_arguments_and_reference_only_normalization_fail_closed(self):
        expected=self.fixture();manifest={'cases':[{'id':'stable','arguments':'os get Caption'}]}
        actual=copy.deepcopy(expected);actual['profile']['oem_codepage']=65001
        with self.assertRaises(ValueError):run_corpus.compare_suite(manifest,expected,actual)
        actual=copy.deepcopy(expected);actual['cases'][0]['arguments_utf16_base64']=base64.b64encode('os get Version'.encode('utf-16le')).decode()
        with self.assertRaises(ValueError):run_corpus.compare_suite(manifest,expected,actual)
        expected['cases'][0]['normalizations']=[{'kind':'wmic-command-starttime','stream':'stdout'}]
        with self.assertRaises(ValueError):run_corpus.compare_suite(manifest,expected,self.fixture())
    def test_report_cli_has_meaningful_exit_codes(self):
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'report.json';script=Path(run_corpus.__file__)
            for payload,want in [({'suites':{'query':{'total':1,'passed':['stable'],'differences':{}}}},0),({'suites':{'query':{'total':1,'passed':[],'differences':{'stable':['stdout mismatch']}}}},1),({'suites':{}},2),({'suites':{'query':{'total':2,'passed':['stable'],'differences':{}}}},2)]:
                payload.update(schema_version=1,requested_suites=['reference'],completed=True)
                payload['suites']={('reference' if k=='query' else k):v for k,v in payload['suites'].items()}
                path.write_text(json.dumps(payload))
                result=subprocess.run([sys.executable,str(script),'--check-report',str(path)],capture_output=True,text=True)
                with self.subTest(payload=payload):self.assertEqual(result.returncode,want,result.stdout+result.stderr)

    def test_status_must_be_a_full_unsigned_dword_not_a_boolean(self):
        manifest={'cases':[{'id':'stable','arguments':'os get Caption'}]}
        for status in (False,True,-1,0x100000000):
            for role in ('reference','candidate'):
                expected=self.fixture();actual=self.fixture()
                (expected if role=='reference' else actual)['cases'][0]['exit_status']=status
                with self.subTest(status=status,role=role),self.assertRaises(ValueError):
                    run_corpus.compare_suite(manifest,expected,actual)

    def test_report_requires_real_ids_and_diagnostic_messages(self):
        for passed,differences in [([False],{}),([] ,{'stable':[False]}),([] ,{'stable':['']})]:
            with self.subTest(passed=passed,differences=differences),self.assertRaises(ValueError):
                run_corpus.report_status({'schema_version':1,'completed':True,'requested_suites':['reference'],'suites':{'reference':{'total':1,'passed':passed,'differences':differences}}})

if __name__=='__main__':unittest.main()


class InterruptedReportTests(unittest.TestCase):
    def test_partial_report_never_passes_a_complete_gate(self):
        report={'schema_version':1,'requested_suites':['reference','compatibility'],
                'completed':False,'suites':{'reference':{'total':1,'passed':['stable'],'differences':{}}}}
        for completed in (False,True):
            report['completed']=completed
            with self.subTest(completed=completed),self.assertRaises(ValueError):
                run_corpus.report_status(report)
