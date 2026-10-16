"""CI protects captured oracles and verifies that the matrix never loses behavior silently."""
import json,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from matrix import generate
from run_corpus import SUITES,FOLDERS,compare_suite

class FixtureInventoryTests(unittest.TestCase):
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
