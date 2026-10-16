"""Strict owned-file comparison; only explicitly selected record metadata is volatile."""
import base64
import re
from compare import compare_cases


def normalize_record_timestamp(data):
    # Locate direct RECORD elements with Expat, then replace only byte spans in
    # their opening tags. Reserializing XML would hide other byte differences.
    from xml.parsers import expat
    if not data.startswith(b'<WMIRECORD '):
        return data
    parser = expat.ParserCreate()
    stack, spans = [], []
    def start(name, attributes):
        if stack == ['WMIRECORD'] and name == 'RECORD':
            at = parser.CurrentByteIndex
            end = data.find(b'>', at)
            match = re.search(rb'\bSTARTTIME="([^"<>]*)"', data[at:end])
            if match:
                spans.append((at+match.start(1), at+match.end(1)))
        stack.append(name)
    def end(name):
        stack.pop()
    def doctype(*args):
        raise ValueError('record DTD is not allowed')
    parser.StartElementHandler, parser.EndElementHandler = start, end
    parser.StartDoctypeDeclHandler = doctype
    try:
        parser.Parse(data, True)
    except (expat.ExpatError, ValueError):
        return data
    for first,last in reversed(spans):
        data = data[:first]+b'{STARTTIME}'+data[last:]
    return data


def compare_owned(payload):
    differences = []
    if payload.get('cleanup') is not True or payload.get('errors'):
        differences.append('owned fixture cleanup/errors: '+repr(payload.get('errors')))
    reference = {c['id']: c for c in payload['cases'] if c['role'] == 'reference'}
    candidates = {c['id']: c for c in payload['cases'] if c['role'] == 'candidate'}
    for name in sorted(reference.keys() | candidates.keys()):
        if name not in reference or name not in candidates:
            differences.append(name+': missing role')
            continue
        expected, actual = reference[name], candidates[name]
        left, right = dict(expected['result']), dict(actual['result'])
        left['normalizations'] = left.get('normalizations') or []
        differences.extend(compare_cases(left, right))
        lf, rf = expected['files'], actual['files']
        for filename in sorted(lf.keys() | rf.keys()):
            if filename not in lf or filename not in rf:
                differences.append(name+': missing file '+filename)
                continue
            a,b=base64.b64decode(lf[filename],validate=True),base64.b64decode(rf[filename],validate=True)
            if filename == 'record.xml' or name == 'record-reset' and filename == 'os':
                a,b=normalize_record_timestamp(a),normalize_record_timestamp(b)
            if a != b:
                first=next((i for i,(x,y) in enumerate(zip(a,b)) if x!=y),min(len(a),len(b)))
                differences.append(f'{name}: file {filename}: {len(a)} != {len(b)} bytes; first difference at {first}')
    return differences
