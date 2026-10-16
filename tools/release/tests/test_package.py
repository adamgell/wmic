import hashlib,json,sys,tempfile,unittest,zipfile
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import package
import struct
def pe(machine=0xAA64):
 data=bytearray(128);data[:2]=b'MZ';struct.pack_into('<I',data,60,64);data[64:68]=b'PE\0\0';struct.pack_into('<H',data,68,machine);return bytes(data)
class PackagingTests(unittest.TestCase):
 def test_allowlist_archive_integrity_and_manifest(self):
  with tempfile.TemporaryDirectory() as tmp:
   root=Path(tmp);binary=root/'wmic.exe';binary.write_bytes(pe())
   (root/'passwords.txt').write_text('do not include');(root/'WMIC-original.exe').write_bytes(b'MZ original')
   readme=root/'README.md';readme.write_text('Usage');coverage=root/'compatibility.md';coverage.write_text('Verified scope')
   archive=package.build_package('aarch64-pc-windows-msvc',binary,root/'out',readme,coverage)
   with zipfile.ZipFile(archive) as z:
    self.assertEqual(set(z.namelist()),{'wmic.exe','README.md','compatibility.md','manifest.json','SHA256SUMS'})
    self.assertEqual(z.read('wmic.exe'),binary.read_bytes());m=json.loads(z.read('manifest.json'))
    for name,v in m['files'].items():self.assertEqual(hashlib.sha256(z.read(name)).hexdigest(),v['sha256']);self.assertEqual(len(z.read(name)),v['bytes'])
    self.assertEqual(m['target'],'aarch64-pc-windows-msvc')
   self.assertEqual(package.verify_package(archive),[])
 def test_reference_paths_are_rejected(self):
  with tempfile.TemporaryDirectory() as tmp:
   root=Path(tmp);p=root/'.reference-bin'/'wmic.exe';p.parent.mkdir();p.write_bytes(b'MZ')
   with self.assertRaises(ValueError):package.build_package('i686-pc-windows-msvc',p,root/'out',p,p)
 def test_tampered_archive_does_not_verify(self):
  with tempfile.TemporaryDirectory() as tmp:
   root=Path(tmp);p=root/'wmic.exe';p.write_bytes(pe(0x8664));doc=root/'README.md';doc.write_text('x')
   archive=package.build_package('x86_64-pc-windows-msvc',p,root/'out',doc,doc)
   other=root/'tampered.zip'
   with zipfile.ZipFile(archive) as source,zipfile.ZipFile(other,'w') as dest:
    for name in source.namelist():dest.writestr(name,b'changed' if name=='wmic.exe' else source.read(name))
   self.assertTrue(package.verify_package(other))
 def test_unknown_target_is_rejected(self):
  with self.assertRaises(ValueError):package.build_package('linux',Path('x'),Path('y'),Path('r'),Path('c'))
