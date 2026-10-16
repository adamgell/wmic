#!/usr/bin/env python3
"""Run the explicitly selected, owned-object/script/file/private-console comparisons."""
import argparse
import base64
import hashlib
import json
import subprocess
import sys
import uuid
from pathlib import Path
from parallels import run_guest_script
from native_tests import stage_artifact
from compare import compare_cases
from destinations import compare_owned as compare_destinations
from controlled import compare_owned as compare_lifecycle
from run_corpus import REFERENCES

DEFINITIONS = [('redirection', 'redirected-output.ps1', '', 11),
 ('scripts', 'unchanged-scripts.ps1', '', 17),
 ('files', 'owned-output.ps1', '', 22),
 ('unicode', 'owned-unicode.ps1', '', 7),
 ('session-files', 'owned-session-output.ps1', '', 9),
 ('formats', 'controlled-formats.ps1', '', 12),
 ('whitespace-formats', 'controlled-formats.ps1', ' -Whitespace', 12),
 ('lifecycle', 'controlled-writes.ps1', '', 11),
 ('named-lifecycle', 'controlled-writes.ps1', ' -NamedMethodInput', 11),
 ('console', 'console-capture.ps1', '', 9)]

EXPECTED = {'console': ['console-break-key',
             'console-cancel-repeat',
             'console-cancel-second',
             'console-context',
             'console-empty',
             'console-eof',
             'console-query',
             'console-repeat2',
             'console-repeat2-wait'],
 'files': ['append-existing',
           'append-invalid-property',
           'append-new',
           'append-reset',
           'append-stdout',
           'append-utf16',
           'append-utf8',
           'both-append-output',
           'both-output-append',
           'output-csv',
           'output-existing',
           'output-invalid-property',
           'output-is-directory',
           'output-missing-dir',
           'output-new',
           'output-reset',
           'output-stdout',
           'record-error',
           'record-existing',
           'record-new',
           'record-output',
           'record-reset'],
 'formats': ['owned-format-basicxml_after',
             'owned-format-basicxml_before',
             'owned-format-csv',
             'owned-format-hform',
             'owned-format-htable',
             'owned-format-mof',
             'owned-format-nocomma_after',
             'owned-format-nocomma_before',
             'owned-format-rawxml',
             'owned-format-table',
             'owned-format-value',
             'owned-format-xml'],
 'session-files': ['repeat-output',
                   'repeat-record',
                   'shell-append',
                   'shell-inline-output',
                   'shell-output',
                   'shell-output-bad',
                   'shell-output-quit',
                   'shell-record',
                   'shell-record-bad'],
 'unicode': ['owned-format-append_ansi',
             'owned-format-append_new',
             'owned-format-csv',
             'owned-format-output',
             'owned-format-record',
             'owned-format-table',
             'owned-format-value'],
 'lifecycle': ['environment-create',
               'environment-delete',
               'environment-delete-declined',
               'environment-delete-help',
               'environment-get',
               'environment-get-changed',
               'environment-set',
               'environment-set-declined',
               'process-create',
               'process-owner',
               'process-terminate'],
 'named-lifecycle': ['environment-create',
                     'environment-delete',
                     'environment-delete-declined',
                     'environment-delete-help',
                     'environment-get',
                     'environment-get-changed',
                     'environment-set',
                     'environment-set-declined',
                     'process-create',
                     'process-owner',
                     'process-terminate'],
 'scripts': ['array-count.ps1',
             'conditional-error.cmd',
             'conditional-success.cmd',
             'csv-properties.ps1',
             'csv.ps1',
             'empty-selection.ps1',
             'error.cmd',
             'error.ps1',
             'for-f.cmd',
             'hardcoded.cmd',
             'if-errorlevel.cmd',
             'parenthesized.cmd',
             'pipe.cmd',
             'query.cmd',
             'query.ps1',
             'redirect.cmd',
             'value-pipeline.ps1'],
 'whitespace-formats': ['owned-format-basicxml_after',
                        'owned-format-basicxml_before',
                        'owned-format-csv',
                        'owned-format-hform',
                        'owned-format-htable',
                        'owned-format-mof',
                        'owned-format-nocomma_after',
                        'owned-format-nocomma_before',
                        'owned-format-rawxml',
                        'owned-format-table',
                        'owned-format-value',
                        'owned-format-xml'],
 'redirection': ['redirect-table',
                 'redirect-value',
                 'redirect-csv',
                 'redirect-query-error',
                 'redirect-help',
                 'redirect-invalid-global',
                 'redirect-repeat',
                 'redirect-shell-query',
                 'redirect-shell-error',
                 'redirect-append',
                 'redirect-method']}

EXPECTED_FILES = {'files': {'output-new': ['out.txt', 'unrelated.bin'],
           'output-existing': ['out.txt', 'unrelated.bin'],
           'append-new': ['append.txt', 'unrelated.bin'],
           'append-existing': ['append.txt', 'unrelated.bin'],
           'both-output-append': ['append.txt', 'out.txt', 'unrelated.bin'],
           'both-append-output': ['append.txt', 'out.txt', 'unrelated.bin'],
           'output-stdout': ['unrelated.bin'],
           'append-stdout': ['unrelated.bin'],
           'output-reset': ['os', 'unrelated.bin'],
           'append-reset': ['unrelated.bin'],
           'append-invalid-property': ['append.txt', 'unrelated.bin'],
           'output-invalid-property': ['out.txt', 'unrelated.bin'],
           'output-missing-dir': ['unrelated.bin'],
           'output-is-directory': ['unrelated.bin'],
           'record-new': ['record.xml', 'unrelated.bin'],
           'record-existing': ['record.xml', 'unrelated.bin'],
           'record-output': ['out.txt', 'record.xml', 'unrelated.bin'],
           'record-error': ['record.xml', 'unrelated.bin'],
           'record-reset': ['os', 'unrelated.bin'],
           'output-csv': ['out.txt', 'unrelated.bin'],
           'append-utf16': ['append.txt', 'unrelated.bin'],
           'append-utf8': ['append.txt', 'unrelated.bin']},
 'unicode': {'owned-format-append_ansi': ['append.txt'],
             'owned-format-append_new': ['append.txt'],
             'owned-format-csv': [],
             'owned-format-output': ['out.txt'],
             'owned-format-record': ['record.xml'],
             'owned-format-table': [],
             'owned-format-value': []},
 'session-files': {'shell-output': ['out.txt', 'unrelated.bin'],
                   'shell-output-quit': ['out.txt', 'unrelated.bin'],
                   'shell-output-bad': ['out.txt', 'unrelated.bin'],
                   'shell-record-bad': ['record.xml', 'unrelated.bin'],
                   'shell-append': ['append.txt', 'unrelated.bin'],
                   'shell-record': ['record.xml', 'unrelated.bin'],
                   'shell-inline-output': ['out.txt', 'unrelated.bin'],
                   'repeat-output': ['out.txt', 'unrelated.bin'],
                   'repeat-record': ['unrelated.bin']},
 'redirection': {'redirect-table': ['stdout.bin', 'stderr.bin'],
                 'redirect-value': ['stdout.bin', 'stderr.bin'],
                 'redirect-csv': ['stdout.bin', 'stderr.bin'],
                 'redirect-query-error': ['stdout.bin', 'stderr.bin'],
                 'redirect-help': ['stdout.bin', 'stderr.bin'],
                 'redirect-invalid-global': ['stdout.bin', 'stderr.bin'],
                 'redirect-repeat': ['stdout.bin', 'stderr.bin'],
                 'redirect-shell-query': ['stdout.bin', 'stderr.bin'],
                 'redirect-shell-error': ['stdout.bin', 'stderr.bin'],
                 'redirect-append': ['stdout.bin', 'stderr.bin'],
                 'redirect-method': ['stdout.bin', 'stderr.bin']}}

def compare_payload(name,payload,count):
    changes=[]
    if count != len(EXPECTED[name]):raise ValueError('incorrect expected case count')
    if any(c.get('role') not in ('reference','candidate') for c in payload['cases']):changes.append('unknown case role')
    indexes=[]
    for role in ['reference','candidate']:
        cases=[c for c in payload['cases'] if c['role']==role];index={c['id']:c for c in cases}
        if len(index)!=len(cases) or set(index)!=set(EXPECTED[name]):changes.append(role+': missing, duplicate or unexpected cases')
        indexes.append(index)
    refs,candidates=indexes
    if changes:return changes
    for case in payload['cases']:
        result=case if name=='console' else case['result']
        status=result.get('exit_status')
        if result.get('state')!='completed' or type(status) is not int or not 0<=status<=0xffffffff:changes.append(case['role']+': '+case['id']+': incomplete process or invalid DWORD status')
        if name=='scripts':
            wanted_status=0x80041017 if case['id'] in ('error.cmd','error.ps1') else 0
            if status!=wanted_status:changes.append(case['role']+': '+case['id']+': script did not reach its expected outcome')
        try:
            if name=='console':
                screen=base64.b64decode(result['screen_utf16_base64'],validate=True)
                if not screen or len(screen)%2 or result['termination_status'] is not None or type(result['width']) is not int or result['width']<=0 or type(result['cursor_x']) is not int or not 0<=result['cursor_x']<result['width'] or type(result['cursor_y']) is not int or result['cursor_y']<0:raise ValueError('invalid console fields')
            else:
                if result['id']!=case['id']:raise ValueError('different result ID')
                arguments=base64.b64decode(result['arguments_utf16_base64'],validate=True)
                if len(arguments)%2:raise ValueError('invalid UTF16 arguments')
                for stream in ('stdout_base64','stderr_base64'):base64.b64decode(result[stream],validate=True)
                if name in ('files','unicode','session-files','redirection'):
                    if not isinstance(case['files'],dict) or set(case['files'])!=set(EXPECTED_FILES[name][case['id']]):raise ValueError('missing or unexpected file inventory')
                    for value in case['files'].values():base64.b64decode(value,validate=True)
        except (KeyError,ValueError,TypeError) as error:
            changes.append(case['role']+': '+case['id']+': missing or invalid evidence: '+str(error))
    if changes:return changes
    if 'lifecycle' in name:
        wanted={'process-create-null-directory'} if name=='named-lifecycle' else set()
        for role in ('reference','candidate'):
            probes=[c for c in payload.get('probes',[]) if c.get('role')==role]
            if len(probes)!=len(wanted) or {c['id'] for c in probes}!=wanted:changes.append(role+': missing or unexpected negative probes')
            for probe in probes:
                try:
                    result=probe['result'];status=result['exit_status']
                    if result['state']!='completed' or type(status) is not int or not 0<=status<=0xffffffff or result['id']!=probe['id']:raise ValueError('incomplete probe or invalid status/ID')
                    arguments=base64.b64decode(result['arguments_utf16_base64'],validate=True)
                    if len(arguments)%2:raise ValueError('invalid UTF16 probe arguments')
                    for stream in ('stdout_base64','stderr_base64'):base64.b64decode(result[stream],validate=True)
                except (KeyError,ValueError,TypeError) as error:changes.append(role+': invalid negative probe: '+str(error))
        if changes:return changes
        return changes+compare_lifecycle(payload)
    if payload.get('cleanup') is not True or payload.get('errors'):changes.append('cleanup/errors: '+repr(payload.get('errors')))
    if name in ('files','unicode','session-files','redirection'):return changes+compare_destinations(payload)
    for key in refs.keys() & candidates.keys():
        if name=='console':
            left=dict(refs[key]);right=dict(candidates[key]);left.pop('role');right.pop('role')
            if left!=right:changes.append(key+': console screen/state/cursor differs')
        else:
            expected=dict(refs[key]['result']);expected['normalizations']=expected.get('normalizations') or []
            if name in ('formats','whitespace-formats') and key in ('owned-format-rawxml','owned-format-xml'):
                expected['normalizations']=[{'kind':'wmic-command-starttime' if key.endswith('rawxml') else 'wmic-html-command-starttime','stream':'stdout'}]
            if expected.get('state')!='completed' or candidates[key]['result'].get('state')!='completed':changes.append(key+': incomplete process')
            changes.extend(compare_cases(expected,candidates[key]['result']))
    return changes


def ps_text(value):
    return "'" + str(value).replace("'", "''") + "'"


def run(vm,target,binary,out,suites=None):
    selected=suites or [d[0] for d in DEFINITIONS]
    if len(set(selected))!=len(selected) or any(name not in {d[0] for d in DEFINITIONS} for name in selected):raise ValueError('invalid or duplicate selected suite')
    definitions=[d for d in DEFINITIONS if d[0] in selected]
    if out.exists():raise ValueError('use a fresh output directory')
    path,sha=stage_artifact(vm,binary);out.mkdir(parents=True)
    root=Path(__file__).resolve().parents[2]
    guest_root=r'C:\ProgramData\WmicCompat\controlled-'+uuid.uuid4().hex
    report={'artifact':{'path':path,'sha256':sha,'target':target},'guest_support_directory':guest_root,'requested_suites':selected,'tests':{},'completed':False}
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    setup="$ErrorActionPreference='Stop'\n$null=New-Item -ItemType Directory -Path "+ps_text(guest_root)+"\n"
    support=[('tools/reference/capture.ps1','capture.ps1'),('tools/windows/console-probe.cs','console-probe.cs')]
    support.extend(('tools/windows/'+script,script) for script in dict.fromkeys(d[1] for d in definitions))
    for relative,name in support:
        encoded=base64.b64encode((root/relative).read_bytes()).decode()
        setup+="[IO.File]::WriteAllBytes("+ps_text(guest_root+'\\'+name)+",[Convert]::FromBase64String('"+encoded+"'))\n"
    run_guest_script(vm,setup+'\n',120)
    for name,script,extra,count in definitions:
        source=(root/'tools/windows'/script).read_text()
        args=" -Candidate "+ps_text(path)+" -CandidateSha256 "+ps_text(sha)
        if name=='console':args+=" -SourcePath "+ps_text(guest_root+r'\console-probe.cs')
        else:args+=" -CaptureScriptPath "+ps_text(guest_root+r'\capture.ps1')
        if name=='scripts' and target=='i686-pc-windows-msvc':
            ref,refhash=REFERENCES['x86'];args+=" -Reference "+ps_text(ref)+" -ReferenceSha256 "+ps_text(refhash)
        evidence=out/(name+'.json')
        guest_evidence=guest_root+'\\'+name+'.json'
        source_hash=hashlib.sha256(source.encode()).hexdigest()
        guest_script=guest_root+'\\'+script
        durable="& {\n$ErrorActionPreference='Stop'\ntry {\nif ((Get-FileHash -LiteralPath "+ps_text(guest_script)+" -Algorithm SHA256).Hash -ine "+ps_text(source_hash)+") { throw 'Harness source hash mismatch' }\n$encodedReport = & ([ScriptBlock]::Create([IO.File]::ReadAllText("+ps_text(guest_script)+")))"+args+extra+"\nif ([string]::IsNullOrWhiteSpace([string]$encodedReport)) { throw 'Empty harness report' }\n[IO.File]::WriteAllBytes("+ps_text(guest_evidence)+",[Convert]::FromBase64String([string]$encodedReport))\n[Console]::WriteLine([string]$encodedReport)\n} catch { [Console]::Error.WriteLine($_.Exception.ToString()); exit 2 }\n}\n\n"
        try:
            encoded=run_guest_script(vm,durable,360)
        except RuntimeError as execution_error:
            # Retrieve durable evidence after a lost Tools reply. Never replay mutations.
            # On missing evidence, retain this owned directory for diagnosis.
            try:encoded=run_guest_script(vm,"$ErrorActionPreference='Stop'\n[Convert]::ToBase64String([IO.File]::ReadAllBytes("+ps_text(guest_evidence)+"))\n\n",120,replay_safe=True)
            except RuntimeError as retrieval_error:raise RuntimeError(str(execution_error)+'\nDurable evidence unavailable: '+str(retrieval_error)) from execution_error
        evidence.write_bytes(base64.b64decode(encoded.strip(),validate=True))
        payload=json.loads(evidence.read_bytes())
        if payload.get('sha256','').lower()!=sha:raise ValueError('controlled evidence covers a different candidate')
        changes=compare_payload(name,payload,count)
        report['tests'][name]={'total':count,'differences':changes,'reference_architecture':'x86' if name=='scripts' and target=='i686-pc-windows-msvc' else 'arm64','script_sha256':hashlib.sha256(source.encode()).hexdigest(),'evidence_sha256':hashlib.sha256(evidence.read_bytes()).hexdigest()}
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(target,name,count,changes,flush=True)
    cleanup="$ErrorActionPreference='Stop'\nif ([IO.Directory]::Exists("+ps_text(guest_root)+")) { Remove-Item -LiteralPath "+ps_text(guest_root)+" -Recurse }\nif ([IO.Directory]::Exists("+ps_text(guest_root)+")) { throw 'Support cleanup incomplete' }\n[Console]::WriteLine('Support cleanup verified')\n\n"
    run_guest_script(vm,cleanup,120,replay_safe=True)
    report['completed']=True
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    return int(any(v['differences'] for v in report['tests'].values()))

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--vm',required=True)
    parser.add_argument('--target',required=True,choices=['aarch64-pc-windows-msvc','x86_64-pc-windows-msvc','i686-pc-windows-msvc'])
    parser.add_argument('--binary',required=True,type=Path)
    parser.add_argument('--out',required=True,type=Path)
    parser.add_argument('--suite',action='append',choices=[d[0] for d in DEFINITIONS])
    args=parser.parse_args()
    try:return run(args.vm,args.target,args.binary,args.out,args.suite)
    except (OSError,ValueError,KeyError,TypeError,RuntimeError,subprocess.TimeoutExpired) as error:
        print('invalid or incomplete controlled evidence: '+str(error),file=sys.stderr)
        return 2

if __name__=='__main__':raise SystemExit(main())
