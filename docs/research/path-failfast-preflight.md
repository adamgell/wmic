# PATH metadata before FAILFAST

Classic WMIC parses some direct `PATH` commands by connecting to WMI and reading class metadata before it checks reachability. The replacement previously performed the reachability check first. For example:

```text
wmic /failfast:on /node:0.0.0.0 path Win32_OperatingSystem get Caption
```

Microsoft returns `0x800706ba` and `The RPC server is unavailable.` from parser connection setup. The previous replacement returned `0xabe3` from FAILFAST instead. With `/TRACE:ON`, Microsoft emits the failed parser connection record and a detailed error, with no query, Node or formatter records.

The Windows backend now has a separate metadata-only preflight hook. The Session controller runs it before FAILFAST and returns its error immediately. The request carries only connection settings, the class name and an optional method name. It cannot carry a query, mutation values or a confirmation callback. Preflight never invokes a method or enumerates instances. Errors retain their full Windows status, and diagnostics pass through the same raw UTF-16 and lossy-text password redactor used by TRACE.

## Measured selection rules

The [34-case manifest](../../tests/fixtures/path-failfast-cases.json) and [original captures](../../tests/fixtures/path-failfast-reference/) cover these distinctions:

| Command form | Parser metadata | Consequence |
| --- | --- | --- |
| One node, `PATH Class GET Property` | Connection, proxy security, amended class object, property names | Metadata errors precede FAILFAST |
| One node, `PATH Class GET /format:csv` or GET help | Property metadata | A trailing verb option changes the bare-GET fast path |
| One node, `PATH Class CALL Method` | Connection, proxy security, class and method-signature metadata | Unknown methods fail before execution; the measured `GetOwner` output signature emits one GetNames record |
| Bare `PATH Class`, or GET as the final token | Skipped | FAILFAST and query errors belong to execution; a missing class returns `0x80041010` with Node and formatter records |
| Multiple distinct explicit nodes | Skipped | Each node reaches normal execution; aggregate output formats once, while `/AGGREGATE:OFF` formats each node |
| Repeated spelling of the same node | One effective node after deduplication | Normal single-node preflight applies |
| `PATH Class LIST` | Syntax rejection | `0xabe7`, before backend access |

Static inspection additionally shows that ordinary SET/CREATE assignments and DELETE skip parser metadata; SET/CREATE help use property metadata. No live writes were used to establish this behavior. Tests use a controlled backend and fail if a rejected request reaches execution or confirmation.

Preflight uses the class portion of a keyed object path, not the full instance. Checking the instance would reject a valid empty selection too early. A zero-address keyed-path regression covers this distinction without invoking a method or changing an object.

## Native stages and evidence

For GET metadata the native order is ConnectServer (parserengine.cpp line 4244), CoSetProxyBlanket (4281), GetObject with `WBEM_FLAG_USE_AMENDED_QUALIFIERS` (3865), and GetNames with `WBEM_FLAG_NONSYSTEM_ONLY` (3877). `WBEM_FLAG_ALWAYS` is zero. The parser connection's displayed argument is the namespace alone, including on explicit-node requests. Actual connection setup still targets the selected node.

CALL uses the same first two records. Its class/method enumeration is logger-only; GetNames emits line 4345 for each present signature. The implementation retains completed signature progress when a later metadata operation fails. Cancellation is checked before and between metadata calls and enumeration iterations. An interrupted error becomes quiet `0xc000013a`; cleanup still ends method enumeration and destroys returned SAFEARRAYs.

REA/Ghidra analysis of the pinned ARM64 original identifies the GET chain `0x14002d450` -> `0x14002b7d0` -> `0x14002a258`, CALL metadata at `0x14002b1e8`, and signature inspection at `0x14002beb8`. Assembly confirms the multi-node skip where decompilation omits reachable COM branches. Selected evidence IDs are `ev_2568dbdb8947b5b23d4672611b9db6fc0f496603c04d0f098763cf4b13093597` (property metadata assembly), `ev_38e04126de2dfba60836ca610444b32199b27da74d0de0e56d32c85e5b44d93d` (connection assembly), and `ev_590e580999023007fd2736698f3461914ee2b7525bcd33f20e0314c6e2774e35` (connection pseudocode). Runtime captures corroborate the ordering and exact diagnostics.

The original executable hashes and English runtime profile are the same as the [TRACE investigation](trace-diagnostics.md). Captures retain raw pipe bytes, arguments, hashes, completion state and full DWORD statuses. No stderr normalization is used.

## Validation status on 2026-10-19

Local validation passes 692 Rust tests, 57 reference Python tests and 11 release Python tests, with formatting, host/all-three-target Clippy and all three Windows release builds. The 34 original captures establish the reference behavior. The two Windows cancellation regressions were proven failing before and passing after the fix on the selected VM.

Full replacement comparison is incomplete. The ARM64 candidate matched 106 method-argument and 62 earlier PATH-method cases against fresh originals. Defender then quarantined that candidate as `Trojan:Win32/Bearfoos.A!ml` (SHA-256 `e4874875b0e540ad175d69c3cabcaa4cab56650bb0d2f98a491f93c18e023a70`). Execution stopped. The focused suite and remaining ARM64/x64/x86 comparisons await Defender review; these builds do not have an all-three-target native pass. No Defender settings or quarantine state were changed.

## Regression coverage and bounds

Portable tests compare the captured diagnostics and interactive failure/recovery sequence, assert metadata -> ping -> execution ordering, and check quiet cancellation, password redaction, class-only keyed-path lookup and rejection before method/write execution. Windows CI launches 20 stable error cases through the actual CLI, compares both output streams and full statuses, drains pipes concurrently, and kills/reaps timed-out children. The read-only method, loopback and multi-node cases run on the selected reference machine. Two Windows cancellation unit tests are explicit guest tests because console interruption is process-wide.

Select the focused native suite with `--suite path-failfast` in `tools/reference/run_corpus.py`. The gate rejects incomplete references, inventory/argument changes, incompatible profiles, corrupt streams and status mismatches. See the [testing guide](../testing.md) for the three target architectures and comparison limits.

This change applies the new phase to direct PATH commands with FAILFAST on the native Windows WMI backend. macOS and WSMan explicitly report that metadata-only preflight is unsupported and retain their prior transport behavior; they do not substitute instance enumeration or a full schema lookup. The shared controller recognizes bare/final-token GET and multiple-node bypass forms with FAILFAST either on or off; captured off controls protect the query-phase diagnostics for those forms.

The investigation also exposed a separate existing gap: keyed PATH GET builds WQL using the complete instance path after FROM instead of converting keys into a WHERE clause. Two read-only exploratory probes for an absent process therefore differ from Microsoft. They are recorded outside this passing suite and remain unfixed here. Parameterized method traces, live writes, other Windows/provider versions, languages and every help or cancellation timing remain unverified. This bounded comparison does not establish full WMIC compatibility.
