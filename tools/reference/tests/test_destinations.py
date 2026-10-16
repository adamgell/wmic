import sys
from pathlib import Path
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from destinations import normalize_record_timestamp
class RecordNormalizationTests(unittest.TestCase):
 def test_only_root_timestamp_is_excluded(self):
  prefix=b'<WMIRECORD USER="D\\U"><RECORD SEQUENCENUM="1" ISSUEDFROM="H" STARTTIME="'
  suffix=b'"><OUTPUT><![CDATA[STARTTIME="a"]]></OUTPUT></RECORD></WMIRECORD>'
  self.assertEqual(normalize_record_timestamp(prefix+b'a'+suffix),normalize_record_timestamp(prefix+b'b'+suffix))
  self.assertNotEqual(normalize_record_timestamp(prefix+b'a'+suffix),normalize_record_timestamp(prefix+b'b'+suffix.replace(b'CDATA[STARTTIME="a"',b'CDATA[STARTTIME="b"')))
  self.assertNotEqual(normalize_record_timestamp(b'<DATA>'+prefix+b'a'+suffix),normalize_record_timestamp(b'<DATA>'+prefix+b'b'+suffix))
