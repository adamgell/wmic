"""Strictly compare the two owned-object lifecycles, retaining their original bytes."""
from __future__ import annotations
import base64
import json
from pathlib import Path
import re
import sys
EXPECTED={'environment-create','environment-get','environment-set-declined','environment-set','environment-get-changed','environment-delete-help','environment-delete-declined','environment-delete','process-create','process-owner','process-terminate'}
def compare_owned(payload: dict) -> list[str]:
    differences=[]
    if payload.get('errors'):differences.append('Lifecycle reported errors: '+repr(payload['errors']))
    roles={}
    for role in ('reference','candidate'):
        cleanup=[c for c in payload['cleanup'] if c['role']==role]
        if len(cleanup)!=1:return ['Missing or duplicate cleanup evidence: '+role]
        cleanup=cleanup[0]
        if cleanup['environment_remaining'] or cleanup['process_remaining']:differences.append(role+': cleanup incomplete')
        pid=cleanup['process_id']
        if type(pid)!=int or pid<=0 or not cleanup['creation_date']:return [role+': missing owned process identity']
        cases=[c for c in payload['cases'] if c['role']==role]
        index={c['id']:c for c in cases}
        if len(index)!=len(cases) or set(index)!=EXPECTED:return [role+': incomplete lifecycle cases']
        normalized={}
        for name,c in index.items():
            result=c['result']
            if result['state']!='completed' or result['exit_status']!=0:differences.append(role+': '+name+' did not succeed')
            streams={s:base64.b64decode(result[s+'_base64'],validate=True) for s in ('stdout','stderr')}
            arguments=base64.b64decode(result['arguments_utf16_base64'],validate=True).decode('utf-16le')
            if name=='process-create':
                ids=re.findall(rb'\tProcessId = ([0-9]+);',streams['stdout'])
                if ids!=[str(pid).encode()]:differences.append(role+': Create returned a different process identity')
                streams['stdout']=streams['stdout'].replace(('\tProcessId = '+str(pid)+';').encode(),b'\tProcessId = {OWNED_PID};')
            if name in ('process-owner','process-terminate'):
                expected='process where ProcessId='+str(pid)+' call '+('GetOwner' if name=='process-owner' else 'Terminate')
                if arguments!=expected:differences.append(role+': command selected a different process identity')
                arguments=arguments.replace('ProcessId='+str(pid)+' call ','ProcessId={OWNED_PID} call ')
                path=('Win32_Process.Handle="'+str(pid)+'"').encode()
                if streams['stdout'].count(path)!=1:differences.append(role+': output has a different process identity')
                streams['stdout']=streams['stdout'].replace(path,b'Win32_Process.Handle="{OWNED_PID}"')
            normalized[name]=(arguments,result['state'],result['exit_status'],streams['stdout'],streams['stderr'])
        roles[role]=normalized
    probes={}
    for role in ('reference','candidate'):
        entries=[c for c in payload.get('probes',[]) if c['role']==role]
        index={c['id']:c for c in entries}
        if len(index)!=len(entries):differences.append(role+': duplicate negative probe')
        probes[role]={name:tuple(c['result'][key] for key in ('arguments_utf16_base64','state','exit_status','stdout_base64','stderr_base64')) for name,c in index.items()}
    if probes['reference']!=probes['candidate']:differences.append('Negative method probe arguments/state/status/stream bytes differ')
    for name in sorted(EXPECTED):
        if roles['reference'][name]!=roles['candidate'][name]:differences.append(name+': arguments/state/status/stream bytes differ')
    return differences
if __name__=='__main__':
    differences=compare_owned(json.loads(Path(sys.argv[1]).read_text()))
    print('\n'.join(differences) if differences else 'All owned lifecycle cases match; cleanup verified. Only the recorded owned PID fields are mapped.')
    raise SystemExit(bool(differences))
