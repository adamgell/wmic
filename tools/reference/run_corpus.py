#!/usr/bin/env python3
"""Run hash-pinned WMIC comparisons; exit 0 for exact matches, 1 for differences, 2 for invalid evidence."""
import argparse,base64,json,sys,subprocess
from pathlib import Path
from parallels import capture,write_capture,validate_manifest
from native_tests import stage_artifact
from compare import compare_cases
SUITES=['reference','format','schema','connection','method','help','interactive','fallback','write-help','switch-help','more-help','class-verb-help','alias','syntax','type','node-file','compatibility','switch-values','rea-25','method-arguments']
FOLDERS={'reference':'reference','format':'format-reference','schema':'schema-reference','connection':'connection-reference','method':'method-reference','help':'help-reference','interactive':'interactive-reference'}
PROFILE_KEYS=('computer','identity','culture','ui_culture','ansi_codepage','oem_codepage','console_output_codepage','directory','launcher_architecture','launch_api','output_mode')
REFERENCES={'arm64':(r'C:\Windows\System32\wbem\WMIC.exe','2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C'),'x86':(r'C:\Windows\SysWOW64\wbem\WMIC.exe','4AFE43FF64AD20475D75D9816A269FC03E10C492033B67E60C9886F3512E900E')}


def compare_suite(manifest,expected,actual,expected_root=None,actual_root=None):
    validate_manifest(manifest)
    wanted={c['id']:c for c in manifest['cases']}
    indexes=[]
    for payload in (expected,actual):
        if payload.get('schema_version')!=1:raise ValueError('unknown capture schema')
        cases=payload['cases'];index={c['id']:c for c in cases}
        if len(index)!=len(cases) or set(index)!=set(wanted):raise ValueError('missing, duplicate or unexpected capture cases')
        indexes.append(index)
    for key in PROFILE_KEYS:
        if key not in expected['profile'] or expected['profile'][key]!=actual['profile'].get(key):raise ValueError('incompatible capture profile: '+key)
    passed=[];differences={}
    for name,request in wanted.items():
        left,right=(index[name] for index in indexes)
        raw=request.get('arguments_utf16_base64') or base64.b64encode(request.get('arguments','').encode('utf-16le',errors='surrogatepass')).decode()
        if left.get('arguments_utf16_base64')!=raw or right.get('arguments_utf16_base64')!=raw:raise ValueError('captured invocation differs from manifest: '+name)
        if (left.get('normalizations') or [])!=(request.get('normalizations') or []):raise ValueError('capture normalization differs from manifest: '+name)
        if left.get('state')!='completed':raise ValueError('reference did not complete: '+name)
        for case in (left,right):
            status=case.get('exit_status')
            if case.get('state')=='completed' and (type(status) is not int or not 0<=status<=0xffffffff):raise ValueError('invalid DWORD status: '+name)
        changes=compare_cases(left,right,expected_root,actual_root)
        if changes:differences[name]=changes
        else:passed.append(name)
    return {'passed':passed,'differences':differences,'total':len(wanted)}


def report_status(report):
    suites=report.get('suites')
    if not isinstance(suites,dict) or not suites:raise ValueError('report has no suites')
    requested=report.get('requested_suites')
    if report.get('schema_version')!=1 or report.get('completed') is not True or not isinstance(requested,list) or any(name not in SUITES for name in requested) or len(set(requested))!=len(requested) or set(requested)!=set(suites):raise ValueError('missing requested suites or incomplete run')
    failed=False
    for suite in suites.values():
        passed=suite['passed'];differences=suite['differences'];total=suite['total']
        if type(total) is not int or total<=0 or not isinstance(passed,list) or not isinstance(differences,dict) or len(set(passed))!=len(passed) or set(passed)&set(differences) or len(passed)+len(differences)!=total:raise ValueError('incomplete or inconsistent report')
        if any(not isinstance(name,str) or not name for name in passed+list(differences)):raise ValueError('invalid case ID')
        if any(not isinstance(v,list) or not v or any(not isinstance(message,str) or not message for message in v) for v in differences.values()):raise ValueError('empty difference record')
        failed|=bool(differences)
    return int(failed)


def run(vm,target,binary,out,fixtures,suites,reference_dir=None):
    if out.exists():raise ValueError('use a fresh output directory')
    path,sha=stage_artifact(vm,binary)
    out.mkdir(parents=True)
    artifact={'path':path,'sha256':sha,'target':target}
    (out/'artifact.json').write_text(json.dumps(artifact,indent=2)+'\n')
    arch='x86' if target=='i686-pc-windows-msvc' else 'arm64'
    refpath,refhash=REFERENCES[arch]
    if not suites or len(set(suites))!=len(suites):raise ValueError('missing or duplicate requested suites')
    report={'schema_version':1,'artifact':artifact,'reference_architecture':arch,'cross_architecture':target=='x86_64-pc-windows-msvc','requested_suites':suites,'completed':False,'suites':{}}
    for suite in suites:
        manifest=json.loads((fixtures/(suite+'-cases.json')).read_text());manifest['reference_sha256']=refhash
        if reference_dir:
            root=reference_dir/suite
            reference=json.loads((root/'capture.json').read_text())
            if reference['profile'].get('executable_sha256','').upper()!=refhash:raise ValueError('wrong cached reference identity')
        else:
            reference=capture(vm,refpath,manifest,expected_sha256=refhash,replay_safe=True)
            root=out/('reference-'+suite);write_capture(reference,root)
        actual=capture(vm,path,manifest,role='candidate',expected_sha256=sha,replay_safe=True)
        write_capture(actual,out/suite)
        result=compare_suite(manifest,reference,actual,root)
        report['suites'][suite]=result
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(target,suite,f"{len(result['passed'])}/{result['total']}",result['differences'],flush=True)
    report['completed']=True
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check-report',type=Path);parser.add_argument('--vm');parser.add_argument('--target',choices=['aarch64-pc-windows-msvc','x86_64-pc-windows-msvc','i686-pc-windows-msvc']);parser.add_argument('--binary',type=Path);parser.add_argument('--out',type=Path);parser.add_argument('--fixtures',type=Path,default=Path('tests/fixtures'));parser.add_argument('--suite',action='append',choices=SUITES);parser.add_argument('--reference-dir',type=Path)
    args=parser.parse_args()
    try:
        if args.check_report:return report_status(json.loads(args.check_report.read_text()))
        if not all((args.vm,args.target,args.binary,args.out)):parser.error('--vm, --target, --binary and --out are required')
        return report_status(run(args.vm,args.target,args.binary,args.out,args.fixtures,args.suite or SUITES,args.reference_dir))
    except (OSError,ValueError,KeyError,TypeError,RuntimeError,subprocess.TimeoutExpired) as error:
        print('invalid or incomplete corpus: '+str(error),file=sys.stderr);return 2
if __name__=='__main__':raise SystemExit(main())
