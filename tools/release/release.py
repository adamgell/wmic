#!/usr/bin/env python3
"""Release context checks for the manually dispatched, main-only signing workflow.

Requires Python 3.11 or later. No credentials or private keys are handled here.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
from urllib.parse import quote
import zipfile
import package


def preflight(root,tag,environment):
    if environment.get('GITHUB_REPOSITORY')!='adamgell/wmic':raise ValueError('release repository must be adamgell/wmic')
    if environment.get('GITHUB_EVENT_NAME')!='workflow_dispatch' or environment.get('GITHUB_REF')!='refs/heads/main':raise ValueError('signed releases must be manually dispatched from main')
    sha=environment.get('GITHUB_SHA','')
    if not re.fullmatch('[0-9a-f]{40}',sha):raise ValueError('invalid source commit')
    checkout=subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip()
    if checkout!=sha:raise ValueError('checkout does not match the workflow source commit')
    for key in ('GITHUB_RUN_ID','GITHUB_RUN_ATTEMPT'):
        if not re.fullmatch('[1-9][0-9]*',environment.get(key,'')):raise ValueError('invalid workflow run context')
    with (root/'Cargo.toml').open('rb') as source:version=tomllib.load(source)['package']['version']
    if not isinstance(version,str) or not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?',version):raise ValueError('invalid crate version')
    if tag!='v'+version:raise ValueError('release tag must exactly match Cargo.toml version')
    return {'tag':tag,'version':version,'source_commit':sha,'run_id':environment['GITHUB_RUN_ID'],'run_attempt':environment['GITHUB_RUN_ATTEMPT']}


def regular_file(path,root):
    if not path.is_file() or path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):raise ValueError('release input must be a regular file within its directory')
    return path.read_bytes()


def check_binary(data,target):
    if package.digest(data) in package.REFERENCE_HASHES:raise ValueError('Microsoft reference binaries cannot be signed or released')
    if package.pe_machine(data)!=package.TARGETS[target]:raise ValueError('executable architecture mismatch')


def stage(binary,target,out,context):
    data=regular_file(binary,binary.parent);check_binary(data,target)
    directory=out/target;directory.mkdir(parents=True,exist_ok=False)
    (directory/'wmic.exe').write_bytes(data)
    metadata={**context,'target':target,'sha256':package.digest(data),'bytes':len(data)}
    (directory/'build.json').write_text(json.dumps(metadata,indent=2)+'\n')
    return metadata


def verify_inputs(directory,context):
    if {p.name for p in directory.iterdir()}!=set(package.TARGETS):raise ValueError('expected exactly three architecture directories')
    for target in package.TARGETS:
        root=directory/target
        if root.is_symlink() or {p.name for p in root.iterdir()}!={'wmic.exe','build.json'}:raise ValueError('unexpected unsigned artifact inventory')
        data=regular_file(root/'wmic.exe',directory);metadata=json.loads(regular_file(root/'build.json',directory));check_binary(data,target)
        if any(metadata.get(k)!=v for k,v in context.items()) or metadata.get('target')!=target or metadata.get('sha256')!=package.digest(data) or metadata.get('bytes')!=len(data):raise ValueError('unsigned artifact provenance or integrity mismatch')
    return {'verified_targets':list(package.TARGETS),**context}


def verify_bundle(directory,context,publisher):
    expected={'release-context.json'}
    for target in package.TARGETS:
        name=f"wmic-compat-{context['version']}-{target}.zip"
        expected.update({name,name+'.sha256',target+'.signature.json'})
    if {p.name for p in directory.iterdir()}!=expected:raise ValueError('incomplete or unexpected release bundle')
    if json.loads(regular_file(directory/'release-context.json',directory))!=context:raise ValueError('release bundle context mismatch')
    for target in package.TARGETS:
        archive=directory/f"wmic-compat-{context['version']}-{target}.zip"
        checksum=regular_file(archive.with_suffix('.zip.sha256'),directory).decode('ascii')
        if checksum!=f'{package.digest(regular_file(archive,directory))}  {archive.name}\n':raise ValueError('archive sidecar mismatch')
        errors=package.verify_package(archive)
        if errors:raise ValueError('; '.join(errors))
        receipt_path=directory/(target+'.signature.json')
        regular_file(receipt_path,directory)
        with zipfile.ZipFile(archive) as zipped:
            manifest=json.loads(zipped.read('manifest.json'));binary=zipped.read('wmic.exe');check_binary(binary,target)
        signing=package.verified_receipt(receipt_path,binary,target,context['source_commit'])
        if manifest.get('target')!=target or manifest.get('version')!=context['version'] or manifest.get('source_commit')!=context['source_commit'] or manifest.get('signing')!=signing:raise ValueError('signed package metadata mismatch')
        if signing['publisher']!=publisher or signing['run_id']!=context['run_id'] or signing['run_attempt']!=context['run_attempt']:raise ValueError('signed package publisher/run mismatch')
    return {'verified_archives':3,**context}


def verify_available(context):
    # Require successful complete API reads. A transport/auth failure is not absence.
    base='repos/adamgell/wmic/'
    releases=json.loads(subprocess.check_output(['gh','api','--paginate','--slurp',base+'releases?per_page=100'],text=True))
    refs=json.loads(subprocess.check_output(['gh','api','--paginate','--slurp',base+'git/matching-refs/tags/'+quote(context['tag'],safe='')],text=True))
    for pages,key,value in [(releases,'tag_name',context['tag']),(refs,'ref','refs/tags/'+context['tag'])]:
        if not isinstance(pages,list) or not pages:raise ValueError('missing version inventory')
        for page in pages:
            if not isinstance(page,list):raise ValueError('invalid version inventory page')
            for item in page:
                if not isinstance(item,dict) or not isinstance(item.get(key),str):raise ValueError('invalid version inventory entry')
                if item[key]==value:raise ValueError('release version or tag already exists; refusing to reuse it')
    return {'available_tag':context['tag']}


def main():
    parser=argparse.ArgumentParser(description=__doc__);sub=parser.add_subparsers(dest='command',required=True)
    for name in ('preflight','stage','verify-inputs','verify-bundle','verify-available'):
        command=sub.add_parser(name);command.add_argument('--root',type=Path,default=Path.cwd());command.add_argument('--tag',required=True)
        if name=='stage':
            command.add_argument('--target',choices=package.TARGETS,required=True);command.add_argument('--binary',type=Path,required=True);command.add_argument('--out',type=Path,required=True)
        if name in ('verify-inputs','verify-bundle'):command.add_argument('--directory',type=Path,required=True)
        if name=='verify-bundle':command.add_argument('--publisher',required=True)
    args=parser.parse_args()
    try:
        context=preflight(args.root,args.tag,os.environ)
        if args.command=='stage':result=stage(args.binary,args.target,args.out,context)
        elif args.command=='verify-inputs':result=verify_inputs(args.directory,context)
        elif args.command=='verify-bundle':result=verify_bundle(args.directory,context,args.publisher)
        elif args.command=='verify-available':result=verify_available(context)
        else:result=context
        print(json.dumps(result));return 0
    except (OSError,ValueError,KeyError,TypeError,zipfile.BadZipFile,subprocess.SubprocessError) as error:parser.exit(1,str(error)+'\n')
if __name__=='__main__':raise SystemExit(main())
