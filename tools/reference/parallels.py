#!/usr/bin/env python3
"""Capture an explicitly identified Windows executable through Parallels Tools."""
from __future__ import annotations

import argparse
import base64
import copy
import hashlib
import json
from pathlib import Path
import re
import subprocess


def validate_manifest(manifest: dict, allow_writes: bool = False) -> None:
    cases = manifest.get("cases")
    if not isinstance(cases, list) or not cases:
        raise ValueError("manifest must contain cases")
    seen = set()
    for case in cases:
        name = case.get("id", "")
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", name) or name in seen:
            raise ValueError("invalid or duplicate case ID: " + repr(name))
        seen.add(name)
        arguments = case.get("arguments", "")
        if "arguments_utf16_base64" in case:
            raw = base64.b64decode(case["arguments_utf16_base64"], validate=True)
            arguments = raw.decode("utf-16le", errors="surrogatepass")
        if re.search(r"(?i)/password\s*:", arguments):
            raise ValueError("credential arguments must not be persisted in captures")
        if case.get("operation", "read_only") != "read_only" and not allow_writes:
            raise ValueError("controlled write case requires explicit selection")


def write_capture(payload: dict, out: Path) -> dict:
    validate_manifest(payload)
    prepared = []
    for case in payload["cases"]:
        result = copy.deepcopy(case)
        if result.get("state") not in ("completed", "timeout", "cancelled"):
            raise ValueError("unknown process state")
        status = result.get("exit_status")
        if result["state"] == "completed":
            if type(status) is not int or not 0 <= status <= 0xFFFFFFFF:
                raise ValueError("completed process requires a full unsigned Windows exit status")
        elif status is not None:
            raise ValueError("timeout/cancel must not masquerade as a process exit")
        streams = {}
        for stream in ("stdout", "stderr"):
            data = base64.b64decode(result.pop(stream + "_base64"), validate=True)
            name = result["id"] + "." + stream + ".bin"
            result[stream] = {"file": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
            streams[name] = data
        prepared.append((result, streams))
    # Validate all cases before creating even the first output artifact.
    out.mkdir(parents=True, exist_ok=True)
    manifest = {key: copy.deepcopy(value) for key, value in payload.items() if key != "cases"}
    manifest["cases"] = []
    for result, streams in prepared:
        for name, data in streams.items():
            (out / name).write_bytes(data)
        manifest["cases"].append(result)
    (out / "capture.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=True) + "\n")
    return manifest


def run_guest_script(vm: str, script: str, timeout: int = 300) -> bytes:
    result = subprocess.run(
        ["prlctl", "exec", vm, "powershell.exe", "-NoProfile", "-NonInteractive", "-Command", "-"],
        input=script.encode("utf-8"), capture_output=True, timeout=timeout,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace") or result.stdout.decode("utf-8", errors="replace"))
    return result.stdout


def capture(vm: str, executable: str, manifest: dict, role: str = "reference", expected_sha256: str | None = None) -> dict:
    validate_manifest(manifest)
    expected = expected_sha256 or manifest.get("reference_sha256", "")
    if not re.fullmatch(r"[a-fA-F0-9]{64}", expected):
        raise ValueError("an exact executable SHA-256 is required")
    source = Path(__file__).with_name("capture.ps1").read_text()
    request = base64.b64encode(json.dumps(manifest, ensure_ascii=True).encode()).decode()
    exe = base64.b64encode(executable.encode("utf-16le", errors="surrogatepass")).decode()
    script = "& {\n" + source + "\n} -ManifestJson ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('" + request + "'))) -Executable ([Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('" + exe + "'))) -ExpectedSha256 '" + expected + "' -Role '" + role + "'\n\n"
    encoded = run_guest_script(vm, script, timeout=manifest.get("timeout_seconds", 300)).strip()
    return json.loads(base64.b64decode(encoded, validate=True).decode("utf-8"))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    command = sub.add_parser("capture")
    command.add_argument("--vm", required=True)
    command.add_argument("--exe", required=True)
    command.add_argument("--manifest", type=Path, required=True)
    command.add_argument("--out", type=Path, required=True)
    command.add_argument("--role", choices=("reference", "candidate"), default="reference")
    command.add_argument("--expected-sha256")
    args = parser.parse_args()
    manifest = write_capture(capture(args.vm, args.exe, json.loads(args.manifest.read_text()), args.role, args.expected_sha256), args.out)
    print(json.dumps({"cases": len(manifest["cases"]), "profile": manifest.get("profile"), "capture": str(args.out / "capture.json")}, indent=2))


if __name__ == "__main__":
    main()
