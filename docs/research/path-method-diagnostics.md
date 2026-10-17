# Unknown PATH method diagnostics

An unknown method on a direct WMI target previously returned the alias error. The corrected behavior distinguishes the original command syntax:

| Command | Status | Stderr before its native line ending |
| --- | --- | --- |
| `path Win32_Process call MissingMethod` | `0xABFC` | `MissingMethod - Invalid class method.` |
| `class Win32_Process call MissingMethod` | `0xABFC` | `MissingMethod - Invalid class method.` |
| `process call MissingMethod` | `0xABFD` | `MissingMethod - Invalid alias verb.` |

The same distinction applies to `/?` and `-?`. Unknown direct method help fails with empty stdout instead of returning an empty help table with status zero. Quoted empty names retain the native leading ` - ` prefix. Missing class and namespace errors still occur before method lookup.

The resolver now retains whether a request came from an alias. That flag survives serialization and instance-path selection; inferring the origin from its query or instance path would misclassify some alias calls. The Windows and macOS backends share the error formatter. Credential-looking method names are withheld through the existing redaction rules, including in help.

Native probes also confirmed that direct method help recognizes fullwidth known names. Help now uses the CLI comparator consistently. Actual Windows invocation still passes the original spelling to WMI, preserving its rejection of fullwidth identifiers. No provider invocation is performed by help.

An invalid namespace exposed another difference: CALL failures inherited query formatting, adding a node header and blank stdout. CALL and CALL help now keep the native method-error presentation. Ordinary CLASS GET retains its node header and empty-result formatting. Both forms have captured controls, and a session test checks that fetching schema for CALL help does not change the error presentation.

## Tests and measurements

The [62-case manifest](../../tests/fixtures/path-method-cases.json) and [original capture](../../tests/fixtures/path-method-reference/capture.json) retain exact UTF-16 invocations, completed statuses and raw stream checksums. Captures use Microsoft WMIC 10.0.26100.1 on the selected Windows 11 ARM64 machine, en-US, SYSTEM, native pipes, ACP 1252 and OEM 437. The original ARM64 and x86 executable identities are checked by the runner.

[path_method_reference.rs](../../tests/path_method_reference.rs) launches the actual executable for every committed case in ordinary Windows CI. It checks the complete DWORD status and both streams, drains the pipes concurrently, and kills/reaps timed-out children. The test uses the same no-console creation flag as the reference capture, so a shell-selected console code page cannot change its diagnostic bytes.

Portable tests cover unknown and fullwidth method help and redaction. Real macOS backend tests preserve alias/direct errors across request serialization; CLI tests verify empty stdout, exact LF stderr, and the POSIX statuses 252 and 253. These checks exercise the built program without invoking a destructive method.

The [validation summary](evidence/2026-10-17/path-method/summary.json) identifies measured implementation commit `f861201`, the source/test/tool patches, candidate executables, reference profiles, report hashes and local checks.

| Build | New diagnostic cases | Broader comparison | Strict status |
| --- | ---: | ---: | ---: |
| ARM64 | 62/62 | 2,250/2,253 across all 21 suites | 1 |
| x64 | 62/62 | 430/430 across six focused suites | 0 |
| x86 | 62/62 | 430/430 across six focused suites | 0 |

The six focused suites are `path-method`, `method-arguments`, `class-verb-help`, `alias`, `method` and `rea-25`. ARM64 execution is native; x64 and x86 use Windows emulation. x64 compares with the ARM64 original, and x86 compares with fresh captures of the x86 original. The ARM64 full report retains the two changing OS-query rows and implementation-specific TRACE difference; its strict gate remains 1. There are no additional differences in that run.

Local verification passes 431 host Rust tests, 57 reference Python tests and 11 release Python tests, formatting, host and all-three-target Clippy, and all three Windows release builds. All six GitHub CI jobs pass on the measured implementation commit, including the Windows CLI comparisons. The session connection-error regression also runs separately on native Windows ARM64.

To repeat the focused comparison:

```sh
python3 tools/reference/run_corpus.py \
  --vm '<reference-vm-id>' \
  --target aarch64-pc-windows-msvc \
  --binary '<absolute-path-to-wmic.exe>' \
  --suite path-method \
  --out '<fresh-output-directory>'
```

## Separate gaps

Six earlier exploratory differences remain outside this fix: three unknown methods combined with missing named values, an unknown method combined with an invalid verb switch, and two singleton-path calls. Their native argument-error precedence or object-path validation needs separate work. The evidence summary retains their baseline results and does not count them as passes.

An additional alias CALL control without WHERE exposed a separate known-method dispatch difference. Microsoft WMIC attempts a class method and returns `0x8004102F`; both the main-branch baseline and fixed build enumerate instances instead. The summary records the complete command and both build identities. This observation is separate from the 62-case suite and is not counted as passing. The two additional CLASS connection controls match.

The existing changing OS-query data and implementation-specific TRACE output remain visible in the full strict report. Measurements cover one Windows version, locale and permission profile, without claiming other Windows releases/locales, native x64/x86 hardware, authenticated remote endpoints or destructive method effects. No power operation, Win32_Product inventory or unrelated account/service/security change was performed.
