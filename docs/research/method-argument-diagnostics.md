# Method-argument diagnostics

The malformed-argument gap recorded after PR #7 returned the wrong diagnostic:

```text
process where "ProcessId=0" call ＧｅｔＯｗｎｅｒ Unknown=1
```

Microsoft WMIC returns `0xB0AB`, `Unknown - Not a parameter of the verb or method.` The previous candidate returned `0xADE5`, `Unknown - Not a input parameter.` This follow-up preserves the distinction and validates named arguments before help and provider method lookup.

## Measured rules

WMIC orders named parameters by their supplied UTF-16 code units before validation. It then checks whether each name exists and whether it is an input parameter. There is no blanket rule that an unknown name always wins: `User=x Unknown=1` reports `Unknown`, but adding `Domain=y` reports the output-only `Domain` first. Lookup uses the CLI case/width comparator; the diagnostic retains the supplied spelling, subject to Windows output encoding.

Alias metadata and WMI method metadata have different parameter inventories. `ReturnValue=1` is unknown for the PROCESS alias's GetOwner verb, while direct PATH invocation recognizes it as an output-only parameter. The implementation validates each request against its relevant metadata. SET and CREATE keep their existing property-binding rules.

Assignment whitespace is accepted, including `Unknown = 1`. Missing values, extra unquoted equals signs, and commas in a named list retain their native syntax errors. Whole-quoted `"Unknown=1"` remains positional. The native atom scanner also treats a comma inside a quoted name as a boundary and keeps a quote open when the immediately preceding character is a backslash. These rules are limited to CALL argument handling.

An invalid named parameter still wins over method help and over a fullwidth method name that WMI would reject. A positional argument follows the WMI lookup path: fullwidth GetOwner continues to fail with `WBEM_E_NOT_FOUND`. No canonicalization turns that provider failure into a successful method call.

## Regression suite

The [manifest](../../tests/fixtures/method-arguments-cases.json) contains 106 read-only cases with exact UTF-16 invocations. The [reference capture](../../tests/fixtures/method-arguments-reference/capture.json) records complete statuses, the original executable identity and environment, and raw stdout/stderr checksums. Twenty content-addressed byte files retain the streams. The `source_captures` entries identify the original captures used to assemble this fixture.

[method_arguments.rs](../../tests/method_arguments.rs) checks 64 provider-free cases on every platform with a backend that panics if called. Its ordinary Windows test launches the actual CLI for all 106 cases, drains both pipes concurrently, checks full DWORD statuses and exact bytes, and kills/reaps timed-out children. CI does not need Microsoft's executable installed. Unicode error-token cases remain in the Windows/native comparisons because OEM best-fit conversion requires Windows.

Focused unit tests distinguish known output-only parameters from unknown names, exercise UTF-16 error ordering and assignment syntax, and preserve positional arrays and quoted values. Separate synthetic tests verify that credential-looking parameter names and values never enter stderr or complete RECORD output. Those synthetic credentials are never sent to Microsoft's executable or stored in reference captures.

## Native validation

The [validation summary](evidence/2026-10-17/method-arguments/summary.json) pins source, test and reference-tool patch digests separately, along with candidate executables, capture/report hashes and local checks. Measurements use the selected Windows 11 ARM64 environment, en-US, SYSTEM, native pipes, ACP 1252 and OEM 437. The original version is 10.0.26100.1 with a valid Microsoft signature.

The ARM64 candidate matches all 106 new diagnostics and 2,188 of 2,191 cases across all twenty suites. Its three strict differences are the existing changing OS queries (`schema/os-default`, `schema/os-get-default`) and implementation-specific `connection/trace`; the full-corpus gate returns 1. The 742-case stable matrix, 286 switch-value cases and 237 REA cases all match exactly.

Both x64 and x86 match 360/360 cases across the method-arguments, alias, method and REA suites, including all 106 new diagnostics. Their focused strict gates return 0; the full 2,191-case corpus was measured only on ARM64 for this follow-up.

Local checks pass 425 Rust tests, 56 Python reference tests and 11 release tests, plus formatting, macOS Clippy, Windows Clippy and release builds for all three targets. The independent review found no actionable findings after the parser corrections.

The architecture results and candidate hashes are recorded in the linked summary. ARM64 runs natively. The x64 candidate runs under emulation and compares with the ARM64 original; the x86 candidate runs under emulation and compares with the x86 original. These measurements do not certify native AMD64/x86 hardware, other Windows versions/locales or permission profiles, or authenticated remote endpoints.

Run the focused suite against an explicitly selected reference environment:

```sh
python3 tools/reference/run_corpus.py \
  --vm '<reference-vm-id>' \
  --target aarch64-pc-windows-msvc \
  --binary '<absolute-path-to-wmic.exe>' \
  --suite method-arguments \
  --out '<fresh-output-directory>'
```

## Separate gaps

Exploration also reproduced existing differences outside this diagnostic fix: CLASS with a WHERE selector, PATH's diagnostic for an unknown method, unknown named parameters combined with invalid verb switches, and an unbalanced escaped quote rejected by strict CALL lexing. Those cases are retained separately in the evidence summary and are not counted as passing cases in the new suite. The full-corpus volatile OS-data and implementation-specific TRACE differences also remain visible.

The evidence summary retains 23 exploratory baseline differences outside the final suite. Those earlier observations were not remeasured on the final candidates.

The read-only probes use GetOwner arguments and metadata. They do not invoke a power operation, inventory Win32_Product, or change accounts, services, security settings or unrelated provider objects. This work does not validate destructive method effects or general WMIC compatibility.
