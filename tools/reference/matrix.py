#!/usr/bin/env python3
"""Reproducible read-only command matrix; the Microsoft executable supplies the oracle."""
import argparse,base64,json
from pathlib import Path
from parallels import validate_manifest


def generate():
    cases=[]
    def add(category,arguments,normalizations=(),stdin=None):
        case={'id':f'{category}-{sum(c["category"]==category for c in cases):03}','category':category,'purpose':category.replace('-',' '),'arguments':arguments,'operation':'read_only','timeout_ms':5000}
        if normalizations:case['normalizations']=list(normalizations)
        if stdin is not None:case['stdin_base64']=base64.b64encode(stdin.encode('ascii')).decode()
        cases.append(case)
    formats=['',' /value',' /format:csv',' /format:mof',' /format:rawxml',' /format:xml',' /format:htable',' /format:hform',' /format:texttable',' /format:texttablewsys',' /format:wmiclivalueformat',' /format:table']
    properties=['Caption','Version','BuildNumber','OSArchitecture','Primary','MUILanguages','OtherTypeDescription','Caption,Version','Version,Caption','Caption,Caption']
    for target in ['os','OS','path Win32_OperatingSystem','path Win32_OperatingSystem=@']:
        for selected in properties:
            for output in formats:
                rules=[]
                if 'rawxml' in output:rules=[{'stream':'stdout','kind':'wmic-command-starttime'}]
                elif output==' /format:xml':rules=[{'stream':'stdout','kind':'wmic-html-command-starttime'}]
                add('query-format',f'{target} get {selected}{output}',rules)
    filters=["Version like '10.%'","Version IS NOT NULL","Caption IS NULL","Primary=TRUE","Primary=FALSE","Version >= '0'","Version <> 'never'","Version='wmic_compat_no_match'","Version LIKE 'wmic_compat_no_match%'","NOT (Version='wmic_compat_no_match')","Version IS NOT NULL AND Primary=TRUE","Caption LIKE '%Windows%'"]
    for selector in filters:
        for selected in ['Caption','Caption,Version']:
            for output in formats[:5]:
                rules=[{'stream':'stdout','kind':'wmic-command-starttime'}] if 'rawxml' in output else []
                add('where-filter',f'os where "{selector}" get {selected}{output}',rules)
    stems=['os get Caption,Version','OS GET Caption,Version','Os gEt Caption,Version','path Win32_OperatingSystem get Caption,Version','os get Version,Caption','os get Caption, Version']
    for stem in stems:
        for separator in [' ','  ','\t',' \t ']:
            for padding in ['', '  ', '\t']:
                add('whitespace',padding+separator.join(stem.split(' '))+padding)
    errors=['os get WmicCompatMissingProperty','path WmicCompatMissingClass get Name','wmic_compat_no_alias get Name','/wmic_compat_invalid:1 os get Caption','os get Caption Version','os get Caption /v','os get Caption /invalid','os where','os where "Version=" get Caption','os where "(Version=" get Caption','os get "Caption','os get Caption,','os get ,Caption','os get Caption,,Version','os get Caption /format:wmic_compat_missing','os get Caption /translate:wmic_compat_missing','os get Caption /every:-1','os get Caption /repeat:-1','os /namespace:root\\cimv2 get Caption','/authlevel:bogus context','/implevel:bogus context','/privileges:bogus context','/aggregate:bogus context','/interactive:bogus context','/failfast:bogus context','os where "Version IS NOT NULL OR" get Caption','os where "Version=123" get Caption','os where "Version=\'snowman ☃\'" get Caption','os where "Version=\'日本語\'" get Caption','os get Caption /format:csv /translate:NoComma']
    for command in errors:
        add('error-grammar',command)
    for option,values in [('/aggregate',['on','off']),('/node',['.','localhost']),('/privileges',['enable','disable']),('/authlevel',['default','pktprivacy']),('/implevel',['impersonate','identify'])]:
        for value in values:
            for output in ['',' /value',' /format:csv']:
                add('connection-local',f'{option}:{value} os get Caption,Version{output}')
    sessions=['os get Caption\r\nquit\r\n','os get Version /value\r\nos get Caption /format:csv\r\nexit\r\n','os get MissingProperty\r\nos get Caption\r\nquit\r\n','\r\n\r\nos get Caption\r\nquit\r\n','context\r\nquit\r\n','/aggregate:off\r\ncontext\r\nos get Caption\r\nquit\r\n','/node:.\r\nos get Caption\r\n/node:""\r\ncontext\r\nquit\r\n','/namespace:\\\\root\\cimv2\r\npath Win32_OperatingSystem get Version\r\nquit\r\n','os /?\r\nos get /?\r\nquit\r\n','wmic_compat_missing\r\ncontext\r\nquit\r\n']
    for session in sessions:add('interactive-flow','',stdin=session)
    payload={'schema_version':1,'reference_sha256':'2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C','timeout_seconds':1800,'cases':cases}
    validate_manifest(payload)
    return payload

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',required=True,type=Path);args=parser.parse_args()
    payload=generate();args.out.write_text(json.dumps(payload,indent=2,ensure_ascii=True)+'\n');print(f'{len(payload["cases"])} read-only compatibility cases')
