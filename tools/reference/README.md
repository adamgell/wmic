# WMIC reference tools

These are development tools, not dependencies of the Rust executable.

`parallels.py capture` runs reviewed cases on an explicitly selected guest.
The manifest pins the executable SHA-256. Reference mode also requires a valid
Microsoft Windows signature. Candidate mode accepts an explicitly hashed build
without identifying it as an original reference.

```sh
python3 tools/reference/parallels.py capture --vm VM_UUID --exe 'C:\Windows\System32\wbem\WMIC.exe' --manifest tests/fixtures/reference-cases.json --out artifacts/reference
python3 tools/reference/compare.py tests/fixtures/reference/capture.json artifacts/reference/capture.json
python3 -m unittest discover -s tools/reference/tests -v
```

The Windows helper calls `CreateProcessW` with the recorded UTF-16 command
line, creates independent native stdout/stderr pipes and closes child stdin.
It drains both streams concurrently. A timeout terminates only that spawned
process and records `state=timeout`, `exit_status=null`. Completed processes
retain their complete unsigned Windows exit status. Base64 protects all bytes
while they cross Parallels' terminal transport.

See [the testing guide](../../docs/testing.md) for the 2,085-case corpus, strict exit codes, all-three-target execution, and controlled runner.

Saved stream hashes are verified before comparisons. Comparisons use only
normalization rules explicitly declared by the reference case. The rules replace the raw XML root `COMMAND` element's volatile `STARTTIME`
attribute or its exact HTML metadata representation; it does not change the stored bytes, property values, whitespace,
line endings, or other timestamps. All other cases remain byte comparisons.

Cases that contain password arguments or request writes are excluded from
this default capture mode. Record actual state-changing validation separately
using explicit test-owned objects rather than labeling such cases read-only.

The initial 23 cases were recaptured through `CreateProcessW` on the selected
ARM64 guest and repeated. They match after the one declared XML timestamp
normalization. Console and file-output encodings require their own tests;
these captures establish native pipe behavior only.

Parallels occasionally reports `PrlJob_GetResult` or `PrlJob_GetRetCode: Invalid argument` despite a responsive guest. `run_guest_script(..., replay_safe=True)` permits up to three attempts for only those errors. The default never replays scripts. Hash-verified artifact staging opts in because it is idempotent. `capture(..., replay_safe=True)` requires an explicit caller choice; do not opt in for controlled writes or commands whose side effects are uncertain. Guest application failures are never retried by this mechanism.

Calls using the same VM UUID are serialized across threads and host processes; the lock covers the whole guest call, including explicitly safe retries. This avoids overlapping Parallels Tools jobs losing their result/status. Controlled operations are still never automatically replayed.
