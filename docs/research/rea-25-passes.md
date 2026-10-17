# Twenty-five additional WMIC investigation passes

After PR #6 merged as `0bb50c5b574ee2521edca57665a8d47bed08701e`, this investigation completed 25 distinct compatibility passes. A pass required a question, recovered native evidence, a source comparison, and controlled executable probes or a bounded negative result. Repeated setup calls did not count. The [ledger](evidence/2026-10-17/rea-25/ledger.json) records every pass, its evidence IDs, baseline differences, and resolution.

REA 6.5.0 used Ghidra 12.1.4 to inspect the original ARM64 executable. The session retained 44 evidence records; the [public evidence index](evidence/2026-10-17/rea-25/native-evidence-index.json) also includes the previously recovered dispatcher used in the node-list analysis. The full analysis bundle and snapshot remain local. Microsoft binaries, decompiled function bodies, and PDBs are not redistributed.

| Original | Version | SHA-256 |
| --- | --- | --- |
| ARM64 Microsoft WMIC | 10.0.26100.1 | `2ef794c4940afea56c6d6d3b9bc9e66965bc7333e79c5dcb2def3c0e36f0834c` |
| x86 Microsoft WMIC | 10.0.26100.1 | `4afe43ff64ad20475d75d9816a269fc03e10c492033b67e60c9886f3512e900e` |

Both references had valid Microsoft Windows signatures. Measurements used the existing Windows ARM64 reference environment, en-US, with native pipes, ANSI code page 1252, and OEM code page 437. Ghidra pseudocode is recovered evidence, with inferred types and boundaries; executable behavior was checked separately.

## Pass inventory

| Pass | Question | Measured result and change |
| ---: | --- | --- |
| 1 | Global prefixes and keyword width | Accept `-` as well as `/`, and scoped fullwidth command keywords. |
| 2 | Colon spacing and adjacent switches | Preserve `/node : .` and adjacent switch boundaries without changing presentation spacing. |
| 3 | Global help and invalid values | Match help precedence and selected missing/invalid switch diagnostics. |
| 4 | Namespace expansion | Preserve equal path spelling, remove one trailing separator, and reproduce parent/reset rules. |
| 5 | Role expansion | Apply the shared expansion rules and the measured cross-field namespace-reset quirk. |
| 6 | Node quotes, empty entries and duplicates | Reproduce the legacy quote/comma scanner, local aliases, separator spacing, and command consumption. |
| 7 | Authentication values | Keep symbolic lookup width-sensitive; accept spaced switch colons. |
| 8 | Impersonation values | Bounded negative: all five selected baseline probes matched. |
| 9 | Privilege toggle tokens | Accept measured case/width spellings and switch spacing. |
| 10 | Interactive setting toggles | Match the selected setting/reset/help behavior. |
| 11 | Aggregate toggles | Match fullwidth toggles and repeated assignments. |
| 12 | CONTEXT trailing grammar | Return `0xBBF1` for trailing options; accept both `/?` and `-?` help. |
| 13 | EVERY numbers | Use legacy `wcstoul`, including quoted empty zero, leading whitespace, digit classes and ULONG overflow. |
| 14 | REPEAT numbers and order | Require immediate adjacency to EVERY, preserve the first error, and apply the final valid pair. |
| 15 | GET property quotes | Unquote separately quoted properties while retaining invalid whole-list quoting. |
| 16 | WHERE boundaries | Bounded negative: all seven selected quoted, parenthesized, empty and malformed probes matched. |
| 17 | Direct PATH targets | Bounded negative: all seven selected class/singleton/local-dot/local-host probes matched. |
| 18 | LIST views | Match fullwidth view names and invalid/duplicate view diagnostics. |
| 19 | FORMAT names | Accept fullwidth aliases; distinguish a bare switch, an empty value and a stylesheet filename. |
| 20 | FORMAT parameters | Accept spaced parameter colons and preserve the measured quoted-comma failure. |
| 21 | TRANSLATE syntax | Report an empty switch before attempting table lookup; preserve width-sensitive table names. |
| 22 | ASSOC options | Validate missing/empty filters, unknown options and unexpected positionals before provider execution. |
| 23 | Method help | Reject unknown methods and extra help tokens; match fullwidth method-name help. No power method was invoked. |
| 24 | Projected XML context | Bounded negative: five envelope/data comparisons matched after declared root STARTTIME normalization. |
| 25 | Interactive recovery | Verify EOF/quit, error recovery and setting persistence with the corrected CONTEXT/path behavior. |

Follow-up probes expanded these passes into [237 read-only cases](../../tests/fixtures/rea-25-cases.json). The [reference capture](../../tests/fixtures/rea-25-reference/capture.json) retains complete statuses, UTF-16 invocations, profiles and stream hashes. Ninety distinct content-addressed byte files store all streams without normalizing their contents. The original undeduplicated capture's hash is retained in `source_capture_sha256`.

## Behavior that needs separate comparisons

WMIC's command comparator at `0x140031b98` calls `CompareStringW(LOCALE_USER_DEFAULT, NORM_IGNORECASE | NORM_IGNOREWIDTH, ...)`. WMI identifiers and several metadata lookups use different rules. The implementation therefore keeps the CLI comparator separate from ordinal identifier comparison. Fullwidth CONTEXT, switch names, booleans, format names and method help work in the measured cases. Fullwidth authentication/impersonation symbols, translation-table names and alias names remain rejected where the original rejected them. The portable CLI fallback covers measured fullwidth ASCII and soft-hyphen behavior; it does not claim complete Windows locale collation on macOS/Linux.

EVERY/REPEAT use `msvcrt!wcstoul` rather than the `_wtol` conversion recovered for FAILFAST. The handler requires the end pointer to reach NUL, ignores errno, allows zero for EVERY and rejects zero for REPEAT. Windows loads this fixed system DLL only from System32; the portable implementation models the selected legacy UTF-16 digit/space classes and 32-bit unsigned saturation. An unquoted minus is a tokenizer boundary and fails, while a quoted negative value can convert successfully. Tests use a fake scheduler and cancellation to inspect huge values without real waits or unbounded execution.

The original node scanner can stop a quoted scan at a comma, then consume following command words as part of the node value. Retain quotes around each individual node when needed, for example `/node:"host-a","host-b"`. The scanner remains iterative for long inputs. Password presentation remains deliberately conservative: malformed credential spellings are redacted, and a NODE value that swallowed credential-looking switch text is rejected before it can enter persistent settings or RECORD output. This privacy boundary is tested with synthetic strings, never by capturing credentials from the reference executable.

## Regression coverage and verification

[rea_reference.rs](../../tests/rea_reference.rs) compares 99 control oracles with a backend that panics if called. Its ordinary Windows test launches the real CLI for the same 99 cases, drains stdout/stderr concurrently, checks complete exit statuses, and kills/reaps a child that exceeds its deadline. It runs through the existing Windows CI `cargo test` step without an ignored-test opt-in. The other 138 cases cover real queries, XML, method help, numeric execution and interactive sessions on the selected reference environment.

Focused provider-free tests cover parsing, namespace/role state, format resolution, repetition counts/cancellation, malformed-input handling and credential redaction. Fixture integrity tests require all 237 cases, all 25 pass groups, 99 control selections, completed reference states and the exact recorded stream hashes. The standard corpus now contains 2,085 cases in 19 suites.

The wider corpus also caught three ASSOC option-help regressions; value validation now waits until help is recognized. It exposed a preexisting aggregate-output discrepancy when one node fails and another succeeds. The pre-PR6 executable reproduced the extra six stdout bytes on the current selected-IP fixture. The controller now defers that failed node's empty-table output while collecting aggregate objects; diagnostics and error statuses remain visible. A test covers both error/success orders and the single-failed-node output.

The [validation summary](evidence/2026-10-17/rea-25/summary.json) identifies the tested executables, reports, source patch and local checks. The final candidate comparisons reuse explicitly selected, hash-pinned completed reference captures with matching profiles; cached OS default queries retain their volatile-data differences. ARM64 execution is native. AMD64 execution uses emulation and compares against the ARM64 original; there is no native AMD64 reference on this machine. x86 execution uses emulation and compares against the x86 original.

These are bounded results for one Windows/locale/permission profile. The existing implementation-specific TRACE transcript and volatile OS-data differences remain visible in full-corpus reports. This work does not establish authenticated remote endpoints, other Windows releases/locales, native AMD64/x86 hardware, destructive methods, provider-specific writes or general WMIC certification. No machine power operation, Win32_Product inventory or unrelated service/security change was performed.

To repeat the focused comparison with a freshly built candidate:

```sh
python3 tools/reference/run_corpus.py \
  --vm '<reference-vm-id>' \
  --target aarch64-pc-windows-msvc \
  --binary '<absolute-path-to-wmic.exe>' \
  --suite rea-25 \
  --out '<fresh-output-directory>'
```

Normal CI consumes the committed oracles and does not install or download Microsoft's WMIC executable or REA/Ghidra.
