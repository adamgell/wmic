import copy
import base64
import json
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from controlled import compare_owned
FIXTURE=Path(__file__).resolve().parents[3]/'docs/research/evidence/2026-10-15/controlled-writes.json'
class ControlledTests(unittest.TestCase):
    def payload(self):return json.loads(FIXTURE.read_text())
    def test_owned_process_ids_differ_but_every_other_byte_matches(self):
        self.assertEqual(compare_owned(self.payload()),[])
    def test_provider_failure_is_not_hidden_by_owned_id_substitution(self):
        data=self.payload()
        case=next(c for c in data['cases'] if c['role']=='candidate' and c['id']=='process-owner')
        case['result']['stdout_base64']=base64.b64encode(base64.b64decode(case['result']['stdout_base64']).replace(b'ReturnValue = 0;',b'ReturnValue = 5;')).decode()
        self.assertTrue(compare_owned(data))
    def test_cleanup_failure_is_not_accepted(self):
        data=self.payload();data['cleanup'][0]['environment_remaining']=1
        self.assertTrue(compare_owned(data))
    def test_wrong_process_identity_is_not_accepted(self):
        data=self.payload();data['cleanup'][1]['process_id']+=1
        self.assertTrue(compare_owned(data))

    def test_negative_probe_status_and_bytes_are_also_compared(self):
        data=self.payload()
        left=copy.deepcopy(next(c for c in data['cases'] if c['role']=='reference' and c['id']=='process-create'))
        right=copy.deepcopy(left);right['role']='candidate';right['result']['exit_status']=5
        data['probes']=[left,right]
        self.assertTrue(compare_owned(data))
