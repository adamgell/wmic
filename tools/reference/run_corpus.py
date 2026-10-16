#!/usr/bin/env python3
"""Run a selected, hash-pinned WMIC corpus profile on the authorized Parallels guest."""
import argparse,json
from pathlib import Path
from parallels import capture,write_capture
from native_tests import stage_artifact
from compare import compare_cases
SUITES=['reference','format','schema','connection','method','help','interactive','fallback','write-help','switch-help','more-help','class-verb-help','alias','syntax','type','node-file']
FOLDERS={'reference':'reference','format':'format-reference','schema':'schema-reference','connection':'connection-reference','method':'method-reference','help':'help-reference','interactive':'interactive-reference'}

def run(vm,target,binary,out,fixtures,suites):
    path,sha=stage_artifact(vm,binary)
    out.mkdir(parents=True,exist_ok=True)
    artifact={'path':path,'sha256':sha,'target':target}
    (out/'artifact.json').write_text(json.dumps(artifact,indent=2)+'\n')
    report={'artifact':artifact,'reference_architecture':'x86' if target=='i686-pc-windows-msvc' else 'ARM64','suites':{}}
    for suite in suites:
        manifest=json.loads((fixtures/(suite+'-cases.json')).read_text())
        root=fixtures/FOLDERS.get(suite,suite+'-reference')
        if target=='i686-pc-windows-msvc':
            manifest['reference_sha256']='4AFE43FF64AD20475D75D9816A269FC03E10C492033B67E60C9886F3512E900E'
            reference=capture(vm,r'C:\Windows\SysWOW64\wbem\WMIC.exe',manifest,expected_sha256=manifest['reference_sha256'],replay_safe=True)
            root=out/('reference-'+suite);write_capture(reference,root)
        else:
            reference=json.loads((root/'capture.json').read_text())
        actual=capture(vm,path,manifest,role='candidate',expected_sha256=sha,replay_safe=True)
        write_capture(actual,out/suite)
        expected={case['id']:case for case in reference['cases']};passed=[];differences={}
        for case in actual['cases']:
            changes=compare_cases(expected[case['id']],case,root)
            if changes:differences[case['id']]=changes
            else:passed.append(case['id'])
        report['suites'][suite]={'passed':passed,'differences':differences,'total':len(actual['cases'])}
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(target,suite,f'{len(passed)}/{len(actual["cases"])}',differences,flush=True)
    return report

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--vm',required=True);parser.add_argument('--target',required=True,choices=['aarch64-pc-windows-msvc','x86_64-pc-windows-msvc','i686-pc-windows-msvc']);parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--out',type=Path,required=True);parser.add_argument('--fixtures',type=Path,default=Path('tests/fixtures'));parser.add_argument('--suite',action='append',choices=SUITES)
    args=parser.parse_args();run(args.vm,args.target,args.binary,args.out,args.fixtures,args.suite or SUITES)
if __name__=='__main__':main()
