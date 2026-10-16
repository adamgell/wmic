"""Run an explicitly selected native test executable through Parallels Tools."""
from __future__ import annotations
import argparse
import hashlib
from pathlib import Path
from parallels import run_guest_script

def stage_artifact(vm: str, source: Path, name: str = 'wmic.exe') -> tuple[str,str]:
    source = source.resolve()
    guest_source = '\\\\Mac\\Home\\' + str(source.relative_to(Path.home())).replace('/', '\\')
    sha = hashlib.sha256(source.read_bytes()).hexdigest()
    stage = (Path(__file__).resolve().parents[1] / 'windows/stage-artifact.ps1').read_text()
    script = "& {\n" + stage + "\n} -SourcePath '" + guest_source.replace("'", "''") + "' -ExpectedSha256 '" + sha + "' -FileName '" + name.replace("'", "''") + "'\n\n"
    path = run_guest_script(vm, script, 120, replay_safe=True).decode().strip()
    expected = 'C:\\ProgramData\\WmicCompat\\artifacts\\' + sha.upper() + '\\' + name
    if path != expected:
        raise RuntimeError('Unexpected artifact staging result: ' + path)
    return path,sha

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--vm', required=True)
    parser.add_argument('--source', required=True, type=Path)
    args = parser.parse_args()
    path,sha = stage_artifact(args.vm, args.source, 'backend-contract.exe')
    script = "& {\n& '" + path + "' --ignored --nocapture --test-threads=1\nif ($LASTEXITCODE -ne 0) { throw ('Native tests failed: '+$LASTEXITCODE) }\n}\n\n"
    print(run_guest_script(args.vm, script, 180).decode(), end='')
