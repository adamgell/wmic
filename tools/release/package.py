#!/usr/bin/env python3
"""Build and verify local, allowlisted Windows WMIC compatibility archives."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import zipfile

TARGETS={'aarch64-pc-windows-msvc':0xAA64,'x86_64-pc-windows-msvc':0x8664,'i686-pc-windows-msvc':0x14C}
NAMES={'wmic.exe','README.md','compatibility.md','manifest.json','SHA256SUMS'}
REFERENCE_HASHES={'2ef794c4940afea56c6d6d3b9bc9e66965bc7333e79c5dcb2def3c0e36f0834c','4afe43ff64ad20475d75d9816a269fc03e10c492033b67e60c9886f3512e900e'}

def digest(data):return hashlib.sha256(data).hexdigest()
def pe_machine(data):
    if len(data)<64 or data[:2]!=b'MZ':raise ValueError('binary is not a Windows PE executable')
    offset=struct.unpack_from('<I',data,60)[0]
    if offset+6>len(data) or data[offset:offset+4]!=b'PE\0\0':raise ValueError('invalid PE header')
    return struct.unpack_from('<H',data,offset+4)[0]

def sums(files):return ''.join(f'{digest(data)}  {name}\n' for name,data in sorted(files.items())).encode()

def build_package(target,binary,out,readme,coverage):
    if target not in TARGETS:raise ValueError('unsupported Windows target')
    if '.reference-bin' in binary.resolve().parts:raise ValueError('reference binaries cannot be packaged')
    data=binary.read_bytes()
    if digest(data) in REFERENCE_HASHES:raise ValueError('Microsoft reference executable cannot be packaged')
    if pe_machine(data)!=TARGETS[target]:raise ValueError('PE machine does not match target')
    files={'wmic.exe':data,'README.md':readme.read_bytes().replace(b'](docs/compatibility.md)',b'](compatibility.md)'),'compatibility.md':coverage.read_bytes()}
    manifest={'schema_version':1,'product':'wmic-compat','version':'0.1.0-dev','target':target,'files':{n:{'sha256':digest(v),'bytes':len(v)} for n,v in sorted(files.items())}}
    files['manifest.json']=(json.dumps(manifest,indent=2,sort_keys=True)+'\n').encode()
    files['SHA256SUMS']=sums(files)
    out.mkdir(parents=True,exist_ok=True)
    archive=out/f'wmic-compat-0.1.0-dev-{target}.zip'
    with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as dest:
        for name,value in sorted(files.items()):
            info=zipfile.ZipInfo(name,date_time=(1980,1,1,0,0,0));info.compress_type=zipfile.ZIP_DEFLATED
            info.external_attr=(0o100755 if name=='wmic.exe' else 0o100644)<<16
            dest.writestr(info,value)
    archive.with_suffix('.zip.sha256').write_text(f'{digest(archive.read_bytes())}  {archive.name}\n',encoding='ascii')
    return archive

def verify_package(archive):
    errors=[]
    with zipfile.ZipFile(archive) as z:
        names=z.namelist()
        if len(names)!=len(set(names)) or set(names)!=NAMES:return ['unexpected or duplicate archive members']
        files={n:z.read(n) for n in names}
        manifest=json.loads(files['manifest.json'])
        if manifest.get('target') not in TARGETS:errors.append('unsupported target')
        expected={'wmic.exe','README.md','compatibility.md'}
        if set(manifest.get('files',{}))!=expected:return ['unexpected manifest file inventory']
        for name,metadata in manifest['files'].items():
            if metadata['sha256']!=digest(files[name]) or metadata['bytes']!=len(files[name]):errors.append('integrity failure: '+name)
        if files['SHA256SUMS']!=sums({n:v for n,v in files.items() if n!='SHA256SUMS'}):errors.append('checksum manifest mismatch')
        try:
            if pe_machine(files['wmic.exe'])!=TARGETS.get(manifest.get('target')):errors.append('PE machine mismatch')
        except ValueError as e:errors.append(str(e))
    return errors

def main():
    root=Path(__file__).resolve().parents[2]
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--target',choices=TARGETS);parser.add_argument('--binary',type=Path);parser.add_argument('--out',type=Path);parser.add_argument('--verify',type=Path)
    args=parser.parse_args()
    try:
        if args.verify:
            errors=verify_package(args.verify)
            for error in errors:print(error)
            return int(bool(errors))
        if not all([args.target,args.binary,args.out]):parser.error('--target, --binary and --out are required')
        print(build_package(args.target,args.binary,args.out,root/'README.md',root/'docs/compatibility.md'))
        return 0
    except (OSError,ValueError,KeyError,zipfile.BadZipFile) as e:
        parser.exit(2,str(e)+'\n')
if __name__=='__main__':raise SystemExit(main())
