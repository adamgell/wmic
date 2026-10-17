"""CI protects captured oracles and verifies that the matrix never loses behavior silently."""
import json,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from matrix import generate
from switch_values import generate as switch_values
from run_corpus import SUITES,FOLDERS,compare_suite

class FixtureInventoryTests(unittest.TestCase):
    def test_path_methods_retain_exact_read_only_case_inventory(self):
        """Keep direct-target diagnostics, alias controls and native help probes in CI."""
        fixtures=Path(__file__).resolve().parents[3]/'tests/fixtures'
        cases=json.loads((fixtures/'path-method-cases.json').read_text())['cases']
        self.assertEqual(len(cases),62)
        self.assertEqual(len({c['id'] for c in cases}),62)
        self.assertTrue(all(c['operation']=='read_only' for c in cases))
        for name in ('path','class','alias'):
            self.assertTrue(any(c['id']==name+'-bare' for c in cases))
            self.assertTrue(any(c['id']==name+'-help' for c in cases))

    def test_method_arguments_retain_read_only_diagnostics_and_control_oracles(self):
        """Guard diagnostic coverage, provider-free selections and completed captures."""
        fixtures=Path(__file__).resolve().parents[3]/'tests/fixtures'
        cases=json.loads((fixtures/'method-arguments-cases.json').read_text())['cases']
        self.assertEqual(len(cases),106)
        self.assertEqual(sum(c['controller_oracle'] for c in cases),64)
        self.assertEqual(len({c['id'] for c in cases}),106)
        self.assertTrue(all(c['operation']=='read_only' for c in cases))
        capture=json.loads((fixtures/'method-arguments-reference/capture.json').read_text())
        self.assertTrue(all(c['state']=='completed' and c['exit_status']!=0 for c in capture['cases']))

    def test_rea_passes_retain_all_read_only_cases_and_control_oracles(self):
        fixtures=Path(__file__).resolve().parents[3]/'tests/fixtures'
        cases=json.loads((fixtures/'rea-25-cases.json').read_text())['cases']
        self.assertEqual(len(cases),237)
        self.assertEqual(sum(c['controller_oracle'] for c in cases),99)
        self.assertTrue(all(c['operation']=='read_only' for c in cases))
        self.assertEqual(len({c['id'] for c in cases}),237)
        for number in range(1,26):
            self.assertTrue(any(c['id'].startswith('p%02d-'%number) for c in cases))

    def test_switch_value_matrix_is_reproducible(self):
        fixtures=Path(__file__).resolve().parents[3]/'tests/fixtures'
        payload=switch_values()
        self.assertEqual(payload,json.loads((fixtures/'switch-values-cases.json').read_text()))
        cases=payload['cases']
        self.assertGreaterEqual(len(cases),280)
        self.assertEqual(len(cases),len({c['id'] for c in cases}))
        self.assertTrue(all(c['operation']=='read_only' and c['arguments'].endswith(' context') for c in cases))
    def test_every_committed_reference_stream_matches_its_manifest_and_checksum(self):
        fixtures=Path(__file__).resolve().parents[3]/'tests/fixtures'
        for suite in SUITES:
            manifest=json.loads((fixtures/(suite+'-cases.json')).read_text())
            root=fixtures/FOLDERS.get(suite,suite+'-reference')
            payload=json.loads((root/'capture.json').read_text())
            with self.subTest(suite=suite):self.assertEqual(compare_suite(manifest,payload,payload,root,root)['total'],len(manifest['cases']))
    def test_matrix_is_reproducible_and_has_substantive_unique_cases(self):
        fixtures=Path(__file__).resolve().parents[3]/'tests/fixtures'
        matrix=generate()
        self.assertEqual(matrix,json.loads((fixtures/'compatibility-cases.json').read_text()))
        cases=matrix['cases'];ids={c['id'] for c in cases}
        self.assertEqual(len(ids),len(cases));self.assertGreaterEqual(len(cases),700)
        # Guard functional categories rather than allowing help expansion to inflate the count.
        for category,minimum in [('query-format',400),('where-filter',100),('error-grammar',25),('whitespace',60),('connection-local',25),('interactive-flow',10)]:
            with self.subTest(category=category):self.assertGreaterEqual(sum(c['category']==category for c in cases),minimum)
        self.assertTrue(all(c['operation']=='read_only' for c in cases))

if __name__=='__main__':unittest.main()
