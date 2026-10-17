# Backwards compatibility testing

The compatibility target is unchanged classic WMIC invocation: the command line, selected WMI operation, stdout, stderr, file bytes, complete Windows exit status, and interactive state. A bounded test matrix cannot certify every Windows provider, version, language, or permission profile.

## Tests that run in CI

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3.12 -m unittest discover -s tools/reference/tests
python3.12 -m unittest discover -s tools/release/tests
```

The Windows job additionally builds and checks all three MSVC targets and executes tests on its x64 runner. The Windows-only HTML regression test renders fixed typed data through MSXML and compares it with original WMIC captures; it does not query the runner's OS data. CI installs stable Rust, with the minimum version enforced by `Cargo.toml`.

The Python suite verifies the inventory and integrity of all 1,562 committed oracle cases. It also regenerates the 742-case matrix exactly and tests that the comparison gate rejects missing/duplicate/substituted cases, incompatible profiles, changed arguments, corrupted streams, invalid status types, timeouts, incomplete reports, missing console evidence, failed cleanup, and omitted owned files. Password-bearing command arguments and interactive stdin are rejected before a capture is persisted.

## Functional matrix

`tools/reference/matrix.py` generates `tests/fixtures/compatibility-cases.json`. These additional cases have no help-only expansion:

| Category | Cases | Behavior |
| --- | ---: | --- |
| Query and format combinations | 480 | Alias case, direct class/instance forms, scalar/array/null/boolean data, property order/duplicates, twelve format spellings |
| Filters | 120 | Boolean, NULL, LIKE, relational and compound predicates, matching and empty selections |
| Whitespace | 72 | Case, tabs, padding, commas, and alias/direct-class invocation |
| Error grammar | 30 | Missing operands, bad properties/classes/options, incomplete quotes, Unicode literals, provider and parser errors |
| Local connection settings | 30 | Local node names, aggregation, privileges, authentication, impersonation |
| Interactive flows | 10 | Multiple queries, errors and recovery, context, persistent settings, EOF/quit |
| **Total** | **742** | Raw streams, UTF-16 arguments, completion state, full DWORD status |

The original sixteen suites contain another 820 cases. The committed raw `.bin` streams and capture manifests are the oracle; checksums protect their exact bytes. To regenerate the command matrix without recapturing output:

```sh
python3.12 tools/reference/matrix.py --out tests/fixtures/compatibility-cases.json
```

## Native comparisons

Use an explicitly selected Windows environment with the installed Microsoft executable. The development runner uses Parallels Tools; that is a test transport, not a CLI runtime dependency. It stages candidate binaries into a hash-specific directory and never replaces the system executable or changes machine-wide PATH.

```sh
python3.12 tools/reference/run_corpus.py \
  --vm VM_UUID --target aarch64-pc-windows-msvc \
  --binary target/aarch64-pc-windows-msvc/release/wmic.exe \
  --out artifacts/native-arm64
```

Repeat with `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`, using a new output directory each time. Each run captures a fresh original by default. The x86 candidate uses the installed x86 original. On the measured ARM64 environment, the x64 candidate uses the ARM64 original, and the report explicitly labels this cross-architecture comparison.

For the additional stable matrix alone, add `--suite compatibility`. For an explicitly selected compatible prior capture, use `--reference-dir DIR`, whose children are suite names containing `capture.json` and raw streams. The runner checks the original executable hash, exact case/argument inventory, declared normalization rules, and matching machine/account/locale/code-page/launcher profiles. Cached captures remain unsuitable for volatile OS fields even when their profiles match.

Exit codes are strict: **0** means all selected cases matched; **1** means completed evidence contains differences; **2** means invalid/incomplete evidence or an execution failure. `--check-report FILE` uses the same gate and requires every requested suite plus the final completion marker. A partial report cannot pass. There is no general known-difference allowlist.

The complete original corpus retains three documented differences per architecture: two queries include changing OS data, and `/TRACE` uses implementation-specific diagnostics. Therefore the full strict run exits 1; the separate 742-case stable suite must match exactly. Do not turn an exit 1 into a success merely by discarding the report.

Only explicitly declared XML root COMMAND STARTTIME or its HTML representation is normalized. Owned file comparisons additionally locate direct RECORD timestamps without reserializing XML; lifecycle comparisons map only the independently recorded owned PID. Other bytes, numeric values, encodings, whitespace, ordering and statuses remain strict.

## Scripts, files, owned objects, and consoles

```sh
python3.12 tools/reference/run_controlled.py \
  --vm VM_UUID --target aarch64-pc-windows-msvc \
  --binary target/aarch64-pc-windows-msvc/release/wmic.exe \
  --out artifacts/owned-arm64
```

Run this separately for each architecture, with a fresh directory. Run guest harnesses sequentially to keep the transport and evidence attributable. Calls using the same VM UUID also hold a host lock across the complete guest execution and any explicitly safe retries. The 121 cases comprise 17 unchanged batch/PowerShell scripts, 22 output/append/record cases, 7 Unicode file cases, 9 session-file cases, 24 string/whitespace formats, 22 lifecycle cases, 9 private-console cases, and 11 direct disk-redirection cases. Named lifecycle tests also compare their negative method probe.

Scripts call literal `wmic` through process-local PATH. The harness verifies executable resolution and hashes, and checks script bytes before each launch. PowerShell fixture bytes are fed unchanged to `powershell -Command -`, so the default Restricted file policy cannot turn both sides into matching blocked-script failures. Successful scripts must exit zero; deliberate invalid-property scripts must return the measured HRESULT. Both the guest harness and offline gate also require a nonempty `VERSION=` line from `for-f.cmd` and `SUCCESS` from `conditional-success.cmd`; matching zero statuses alone cannot establish success. No execution policy is changed. Cases include `FOR /F`, pipelines, CSV parsing, conditional commands, ERRORLEVEL/LASTEXITCODE, empty selections, parentheses, and file redirection. A literal system path continues to resolve to Microsoft WMIC. Formatted query stdout redirected to disk is checked separately from native pipe output: disk output uses UTF-16LE with a BOM and CRLF, while help and diagnostics retain their measured byte paths.

Controlled writes use fresh GUID-named environment values, independently identified child processes, owned temporary files, and hidden private consoles. Harnesses verify cleanup and preserve an unrelated file sentinel. They do not inventory `Win32_Product` or change accounts, services, security, power settings, or unrelated provider objects. These tests require explicit selection; the default read-only corpus does not run them.

The controlled runner transfers exact supporting-file bytes into its own unique guest directory, hashes those same bytes, durably writes each result, verifies the measured candidate hash, and downloads evidence before deleting support files. A lost transport reply triggers a read of durable evidence; it never automatically repeats the controlled operation. If no evidence can be retrieved, execution stops and retains the owned support directory for diagnosis. A later run uses a new directory and new owned identities. Repeatable `--suite NAME` selects particular suites for a fresh, explicitly chosen measurement.

The offline aggregation mode accepts only fully completed source runs, including their exact requested-suite inventories. Every contributing suite must validate its candidate/target, current raw harness hash, evidence digest, full case inventory, typed results and fixture cleanup. Partial runs and aggregate reports cannot be used as source runs. The first supplied source for each selected suite wins; unused suites are not counted. Evidence bytes and the source report's path and digest are preserved. For example, combine two independently completed measurements without accessing the guest:

```sh
python3.12 tools/reference/run_controlled.py \
  --aggregate-report artifacts/arm64-scripts/report.json \
  --aggregate-report artifacts/arm64-files/report.json \
  --target aarch64-pc-windows-msvc \
  --binary target/aarch64-pc-windows-msvc/release/wmic.exe \
  --suite scripts --suite files --out artifacts/arm64-combined
```

Without `--suite`, aggregation requires all ten suites. It exits 2 without writing output if any source is incomplete, contributing evidence is invalid, or a requested suite is missing.

Read [the compatibility report](compatibility.md) for exact measured binaries, results, reference profiles, and outstanding platform/provider acceptance. Code signing and test execution establish different facts.
