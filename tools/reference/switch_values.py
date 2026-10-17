"""Read-only CONTEXT cases derived from a captured legacy MSVCRT conversion oracle."""
import argparse
import json
from pathlib import Path


def generate():
    fixtures = Path(__file__).resolve().parents[2] / 'tests/fixtures'
    oracle = json.loads((fixtures / 'legacy-wtol.json').read_text())
    cases, seen = [], set()

    def add(category, command):
        if command in seen:
            return
        seen.add(command)
        cases.append({'id': f'{category}-{len(cases):03d}', 'arguments': command,
                      'operation': 'read_only', 'timeout_ms': 10000})

    def numeric(value):
        add('failfast', f'/failfast:"{value}" context')

    for case in oracle['values']:
        numeric(case['input'])
    for zero in oracle['digit_zero_units']:
        for digit in range(10):
            numeric(chr(zero + digit) + '1')
    for unit in oracle['space_units']:
        numeric(chr(unit) + '125')
    for value in ['\u0085125', '\u200b125', '\ufeff125', '\u0be7125',
                  '\u07c1125', '\U0001d7cf125', '1\u0be7', '1\u200b',
                  '1\u2028', '+\u2003125', '-\u2003125']:
        numeric(value)
    for name, values in [('trace', ['OFF', ' OFF', 'OFF ', 'OFFfoo', '0']),
                         ('aggregate', ['ON', ' ON', 'ON ', 'ONfoo', '1'])]:
        for value in values:
            add(name, f'/{name}:"{value}" context')
    for value in ['ms_409', 'MS_409', 'en-US', 'bogus', '0x409', '1033',
                  'ms_000', 'MS_ffff', 'ms_409x', 'MS_411']:
        add('locale', f'/locale:"{value}" context')
    for prefix in ['/locale:MS_411 /locale:ms_411',
                   '/locale:MS_411 /locale:MS_409 /locale:""',
                   '/locale:MS_411 /locale:""',
                   '/locale:BOGUS /locale:bogus',
                   '/failfast:125ms /failfast:off /failfast:on']:
        add('sequence', prefix + ' context')
    return {'schema_version': 1,
            'reference_sha256': '2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C',
            'timeout_seconds': 600, 'cases': cases}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    payload = generate()
    args.out.write_text(json.dumps(payload, indent=2, ensure_ascii=True) + '\n')
    print(f'{len(payload["cases"])} read-only switch-value cases')
