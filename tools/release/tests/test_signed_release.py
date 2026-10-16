import copy, hashlib, json, os, subprocess, sys, tempfile, unittest, zipfile
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import package
from test_package import pe
SCRIPT=Path(__file__).resolve().parents[1]/'release.py'
SHA='0123456789abcdef0123456789abcdef01234567'

class SignedPackageTests(unittest.TestCase):
    def fixture(self,root):
        binary=root/'wmic.exe';binary.write_bytes(pe())
        doc=root/'README.md';doc.write_text('Development compatibility build')
        receipt={'schema_version':1,'source_commit':SHA,'target':'aarch64-pc-windows-msvc','run_id':'123','run_attempt':'1','file':{'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'bytes':binary.stat().st_size},'verification':{'authenticode_status':'Valid','signature_type':'Authenticode','signtool_exit_code':0,'publisher':'Adam Gell','timestamp_present':True,'signer_thumbprint':'A'*40,'timestamp_thumbprint':'B'*40}}
        path=root/'signature.json';path.write_text(json.dumps(receipt))
        return binary,doc,path,receipt
    def build(self,binary,doc,receipt):
        try:return package.build_package('aarch64-pc-windows-msvc',binary,binary.parent/'out',doc,doc,signature_receipt=receipt,source_commit=SHA)
        except TypeError:self.fail('Signed package provenance is not implemented')
    def test_signed_package_binds_the_verified_bytes_and_publisher(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary,doc,path,receipt=self.fixture(Path(tmp));archive=self.build(binary,doc,path)
            with zipfile.ZipFile(archive) as z:
                manifest=json.loads(z.read('manifest.json'));self.assertEqual(z.read('wmic.exe'),binary.read_bytes())
                self.assertEqual(manifest['source_commit'],SHA)
                self.assertEqual(manifest['signing']['publisher'],'Adam Gell')
                self.assertEqual(manifest['signing']['signature_receipt_sha256'],hashlib.sha256(path.read_bytes()).hexdigest())
            self.assertEqual(package.verify_package(archive),[])
    def test_signed_package_rejects_stale_or_wrong_verification(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary,doc,path,original=self.fixture(Path(tmp))
            for key,value in [('authenticode_status','NotSigned'),('signtool_exit_code',1),('timestamp_present',False),('publisher','')]:
                receipt=copy.deepcopy(original);receipt['verification'][key]=value;path.write_text(json.dumps(receipt))
                with self.subTest(key=key),self.assertRaises(ValueError):self.build(binary,doc,path)
            receipt=copy.deepcopy(original);receipt['file']['sha256']='0'*64;path.write_text(json.dumps(receipt))
            with self.assertRaises(ValueError):self.build(binary,doc,path)
            receipt=copy.deepcopy(original);receipt['source_commit']='f'*40;path.write_text(json.dumps(receipt))
            with self.assertRaises(ValueError):self.build(binary,doc,path)
            receipt=copy.deepcopy(original);receipt['target']='i686-pc-windows-msvc';path.write_text(json.dumps(receipt))
            with self.assertRaises(ValueError):self.build(binary,doc,path)
            self.assertFalse((Path(tmp)/'out').exists())

class ReleaseFixture(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
        (self.root/'Cargo.toml').write_text('[package]\nname="fixture"\nversion="0.1.0-dev"\n')
        subprocess.run(['git','init','--quiet',str(self.root)],check=True)
        subprocess.run(['git','-C',str(self.root),'add','Cargo.toml'],check=True)
        subprocess.run(['git','-C',str(self.root),'-c','user.name=Fixture','-c','user.email=fixture@example.invalid','-c','commit.gpgsign=false','commit','--quiet','-m','fixture'],check=True)
        self.sha=subprocess.check_output(['git','-C',str(self.root),'rev-parse','HEAD'],text=True).strip()
        self.env={**os.environ,'GITHUB_REPOSITORY':'adamgell/wmic','GITHUB_EVENT_NAME':'workflow_dispatch','GITHUB_REF':'refs/heads/main','GITHUB_SHA':self.sha,'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1'}
    def tearDown(self):self.temp.cleanup()
    def run_cli(self,tag='v0.1.0-dev',**changes):
        return subprocess.run([sys.executable,str(SCRIPT),'preflight','--root',str(self.root),'--tag',tag],env={**self.env,**changes},capture_output=True,text=True)
class ReleaseContextTests(ReleaseFixture):
    def test_release_is_bound_to_main_checkout_and_cargo_version(self):
        result=self.run_cli();self.assertEqual(result.returncode,0,result.stderr)
        body=json.loads(result.stdout);self.assertEqual(body['source_commit'],self.sha);self.assertEqual(body['version'],'0.1.0-dev');self.assertEqual(body['tag'],'v0.1.0-dev')
    def test_release_rejects_other_refs_events_repositories_and_sources(self):
        for changes in [{'GITHUB_REF':'refs/pull/1/merge'},{'GITHUB_REF':'refs/heads/feature'},{'GITHUB_EVENT_NAME':'pull_request'},{'GITHUB_REPOSITORY':'fork/wmic'},{'GITHUB_SHA':'f'*40},{'GITHUB_RUN_ID':''}]:
            with self.subTest(changes=changes):self.assertNotEqual(self.run_cli(**changes).returncode,0)
        for tag in ['v0.2.0','v0.1.0-dev\nmalicious','../../release']:
            with self.subTest(tag=tag):self.assertNotEqual(self.run_cli(tag).returncode,0)

class ReleaseBundleTests(ReleaseFixture):
    def command(self,*arguments):
        return subprocess.run([sys.executable,str(SCRIPT),*arguments,'--root',str(self.root),'--tag','v0.1.0-dev'],env=self.env,capture_output=True,text=True)
    def test_unsigned_input_gate_checks_all_targets_and_detects_tampering(self):
        out=self.root/'unsigned'
        for target,machine in [('aarch64-pc-windows-msvc',0xAA64),('x86_64-pc-windows-msvc',0x8664),('i686-pc-windows-msvc',0x14C)]:
            binary=self.root/(target+'.exe');binary.write_bytes(pe(machine))
            result=self.command('stage','--target',target,'--binary',str(binary),'--out',str(out))
            self.assertEqual(result.returncode,0,result.stderr)
        result=self.command('verify-inputs','--directory',str(out));self.assertEqual(result.returncode,0,result.stderr)
        binary=out/'aarch64-pc-windows-msvc'/'wmic.exe';binary.write_bytes(pe(0x8664))
        self.assertNotEqual(self.command('verify-inputs','--directory',str(out)).returncode,0)
    def test_publication_gate_requires_complete_signed_bundle_and_matching_receipts(self):
        out=self.root/'packages';out.mkdir();doc=self.root/'README.md';doc.write_text('Development build')
        for target,machine in [('aarch64-pc-windows-msvc',0xAA64),('x86_64-pc-windows-msvc',0x8664),('i686-pc-windows-msvc',0x14C)]:
            binary=self.root/(target+'.exe');binary.write_bytes(pe(machine))
            receipt={'schema_version':1,'source_commit':self.sha,'target':target,'run_id':'123','run_attempt':'1','file':{'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'bytes':binary.stat().st_size},'verification':{'authenticode_status':'Valid','signature_type':'Authenticode','signtool_exit_code':0,'publisher':'Adam Gell','timestamp_present':True,'signer_thumbprint':'A'*40,'timestamp_thumbprint':'B'*40}}
            path=out/(target+'.signature.json');path.write_text(json.dumps(receipt))
            package.build_package(target,binary,out,doc,doc,signature_receipt=path,source_commit=self.sha)
        context={'tag':'v0.1.0-dev','version':'0.1.0-dev','source_commit':self.sha,'run_id':'123','run_attempt':'1'}
        (out/'release-context.json').write_text(json.dumps(context))
        result=self.command('verify-bundle','--directory',str(out),'--publisher','Adam Gell')
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertNotEqual(self.command('verify-bundle','--directory',str(out),'--publisher','Other').returncode,0)
        receipt=out/'i686-pc-windows-msvc.signature.json';receipt.write_text(receipt.read_text()+' ')
        self.assertNotEqual(self.command('verify-bundle','--directory',str(out),'--publisher','Adam Gell').returncode,0)
