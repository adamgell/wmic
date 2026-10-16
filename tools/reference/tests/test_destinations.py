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
 def test_all_direct_record_clocks_are_excluded_without_touching_cdata(self):
  first=b'<WMIRECORD USER="U"><RECORD SEQUENCENUM="1" ISSUEDFROM="H" STARTTIME="a"><OUTPUT><![CDATA[<RECORD STARTTIME="sensitive">]]></OUTPUT></RECORD>'
  second=b'<RECORD SEQUENCENUM="2" ISSUEDFROM="H" STARTTIME="a"><OUTPUT/></RECORD></WMIRECORD>'
  self.assertEqual(normalize_record_timestamp(first+second),normalize_record_timestamp(first.replace(b'STARTTIME="a"',b'STARTTIME="b"')+second.replace(b'STARTTIME="a"',b'STARTTIME="c"')))
  self.assertNotEqual(normalize_record_timestamp(first+second),normalize_record_timestamp(first.replace(b'sensitive',b'changed')+second))
