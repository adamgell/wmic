# Compatibility status

Development build `0.1.0-dev`, 2026-10-16; measured application baseline source commit `ac390d4`. All 13 planned implementation tasks are delivered. **Full classic WMIC compatibility is not claimed.** This is a measured development replacement with explicit differences and incomplete platform/provider acceptance. The corpus results below apply to the local unsigned baseline. GitHub release builds use a separate build environment and acquire different hashes after signing; their manifests and signature receipts identify their exact bytes, source commit, and workflow run. Signing does not establish additional runtime compatibility.

## Reference profile and architecture matrix

The selected Parallels guest is `Windows 11 Home Domain`, UUID `{ec3bc428-7a31-4f17-8424-7ce78923f7f3}`, hostname `DESKTOP-N4H9E7R`, endpoint `192.168.2.107`. The installed OS is Windows 11 Pro 25H2 ARM64, build 26200.8037, en-US. Captures run as `NT AUTHORITY\SYSTEM`, ACP 1252, OEM/console code page 437. Ordinary-user acceptance is separate and unverified.

Microsoft ARM64 WMIC version 10.0.26100.1 is pinned to SHA256 `2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C`. The installed x86 reference is pinned to `4AFE43FF64AD20475D75D9816A269FC03E10C492033B67E60C9886F3512E900E`.

| Candidate | Runtime | Corpus reference | Final result |
| --- | --- | --- | --- |
| ARM64 | Native on the selected ARM64 guest | Installed ARM64 original | 817/820 match; 3 documented differences |
| x64 | x64 emulation on the ARM64 guest | Installed ARM64 original, cross-architecture comparison | 817/820 match; 3 documented differences |
| x86 | x86 emulation on the ARM64 guest | Fresh installed x86-original captures | 817/820 match; 3 documented differences |

All three MSVC release targets compiled with static CRT linkage. Native AMD64 and native x86 hardware execution are unverified. The controlled scripts/files/methods/console matrix uses the installed ARM64 original for all candidates; the main x86 corpus alone is paired with the installed x86 original. No additional endpoint was accessed for these tests.

## Corpus results

The following is a bounded corpus of 820 cases per candidate, not a percentage of all WMIC behavior. Every matched case compares stdout, stderr, exact UTF16 arguments, completion state and full-DWORD process exit status. The same 3 known differences occur on each architecture.

| Suite | Cases | ARM64 matched | x64 matched | x86 matched |
| --- | ---: | ---: | ---: | ---: |
| Core queries/diagnostics | 23 | 23 | 23 | 23 |
| Formats/XSL/translations | 42 | 42 | 42 | 42 |
| Schema/association/defaults | 31 | 29 | 29 | 29 |
| Connections/security settings | 13 | 12 | 12 | 12 |
| Safe methods/errors/declines | 13 | 13 | 13 | 13 |
| Alias/global contextual help | 344 | 344 | 344 | 344 |
| Redirected interactive sessions | 46 | 46 | 46 | 46 |
| Alias metadata predicates/objects | 11 | 11 | 11 | 11 |
| Alias CREATE/SET/DELETE help | 243 | 243 | 243 | 243 |
| Verb switch help | 9 | 9 | 9 | 9 |
| Additional help surfaces | 10 | 10 | 10 | 10 |
| Class/method schema help | 8 | 8 | 8 | 8 |
| Alias/view/PWhere cases | 4 | 4 | 4 | 4 |
| Syntax/quoting/error cases | 19 | 19 | 19 | 19 |
| Typed properties | 2 | 2 | 2 | 2 |
| Node file inputs | 2 | 2 | 2 | 2 |
| **Total** | **820** | **817** | **817** | **817** |

`os-default` and `os-get-default` retain raw differences in changing OS memory/time/process data; stable headers and status match, but exact equality of these rows is not asserted. `connection/trace` retains an intentional stderr difference: this implementation reports its own activity instead of fabricating Microsoft's C++ trace locations/call sequence. Password redaction is another intentional difference for credential-bearing XML/record/error output; reference corpora prohibit password-bearing captures.

## Controlled effects and scripts

Each candidate completed 100 controlled comparison cases: 7 unchanged scripts, 22 output/append/record cases, 7 Unicode file cases, 9 persistent/inline session-routing cases, 12 string-format cases, 12 whitespace-string-format cases, 11 owned-object lifecycle cases, 11 named-method lifecycle cases, and 9 completed private-console cases. All match under the explicitly declared clock/PID policies below. Negative method probes are also compared within the lifecycle evidence. Every harness checks its own cleanup; output harnesses preserve an unrelated sentinel.

The unchanged `.cmd`/`.ps1` files call literal `wmic`: querying, piping, CSV parsing and error handling work under process-local PATH precedence. Resolution is read back and its executable hash checked. A hardcoded `C:\Windows\System32\wbem\WMIC.exe` call continues to use Microsoft WMIC under either PATH. No system executable or machine-wide PATH was replaced.

Native console cases cover context/query/empty line/EOF, finite repetition, CtrlC before/after another cycle and ordinary-key interruption in new hidden private consoles. A separate ARM64 typeahead case matches the original partial screen/cursor/buffer state; **both programs time out**, have null application exit status, and are forcibly terminated by the harness. It is not counted as a completed success. It confirms the measured legacy consumption of one queued key after a query.

## Regression and implementation coverage

The portable Rust suite passes 158 tests. Native Windows tests pass 18 library and 3 backend tests; Python reference/package suites pass 24+4. Formatting, both Clippy runs and all release cross-builds pass. The one independent whole-branch review found 4 Important issues, all fixed in a single RED-to-GREEN pass: malformed-password diagnostic leakage, cancellation during the final backend call/batch, dropped association partial results, and invalid Unicode-code-page append arguments. No Critical findings or deferred minors remain from that review. There was no second review.

Cancellation requests COM cancellation on the executing thread, stops before later target actions, and retains `0xC000013A`. A native read-only WMI event-enumeration regression proves an outstanding wait unblocks. Cancellation cannot undo a provider-side mutation already accepted. The [COM cancellation contract](https://learn.microsoft.com/en-us/windows/win32/com/canceling-method-calls) does not guarantee that the server stops processing. UTF8/GB18030 append conversion is tested through explicit code-page arguments; the guest's system locale was unchanged, so a complete UTF8-ACP Windows profile is not certified.

The implementation includes raw UTF16 WMIC grammar, live/default-bundled aliases, direct PATH/CLASS/WQL/ASSOC, schema-driven methods/instance and class operations, ordered multiple-node settings, embedded stock formatting and custom XSL/script support, translations, output/append/record/clipboard destinations, interactive context, repetitions and CtrlC handling. The bundle contains 81 aliases and 426 views. Forced missing-default-role tests cover stock alias use and local read-only ALIAS scalar/object introspection. A genuinely missing-`root\cli` machine profile remains unverified; advanced intrinsic/schema/mutation behavior of the virtual alias provider is limited. Missing custom roles return errors.

## Evidence and normalization

Committed source evidence under `docs/research/evidence/2026-10-16` contains per-architecture passed/different case IDs, the controlled-matrix summary, check results and an evidence-integrity index. Original reference streams and manifests remain in `tests/fixtures`. Post-fix raw captures and complete harness results are retained locally under `artifacts/reviewed-conformance-*` and `artifacts/reviewed-*.json`; they are excluded from release packages. The index hashes their capture manifests and harness outputs; stored capture manifests in turn hash every raw stream.

Normalization is opt-in and narrow: root COMMAND STARTTIME in raw XML; its exact root HTML metadata representation; direct RECORD children STARTTIME attributes located by Expat without reserializing; and the exact recorded owned PID in controlled Create/path/selector fields. Nested properties, CDATA, whitespace, encodings, ordering, all other numbers and status remain strict. Volatile OS values are not normalized. The separate `docs/research/implementation-decisions.md` preserves every ruling and its cost; `docs/research/final-review.md` records review dispositions.

The locally measured unsigned baseline executable hashes are:

| Architecture | Target | SHA256 |
| --- | --- | --- |
| arm64 | `aarch64-pc-windows-msvc` | `cf1f6d2dec0ebaa559e373987bb1ce2d2ce62fe6f48049d194932549ecb52248` |
| x64 | `x86_64-pc-windows-msvc` | `7024a58f151713cba12662aaa94444a87dcc5634756e2d4073c3f5bf7dc6e7eb` |
| x86 | `i686-pc-windows-msvc` | `1732bd1a506c7284d369867221b0bd5f4fbaa504f5044b0c6dcb819f8d71afd8` |

## Open acceptance and known limits

Additional authenticated remote endpoints, IPv6 fail-fast, non-en-US Windows, Windows 10/Server, native AMD64/x86 hardware and ordinary-user profiles are unverified. Clipboard APIs and class-repository writes are implemented but live acceptance is unexercised. Destructive methods, account/service/security changes, reboot operations and Win32_Product instance inventory were not exercised. Safe schema/help reads do not certify those effects.

Custom-role localization fallback, arbitrary derived-command parameter substitution/routing, embedded-object CLI argument syntax, and unusual formatting sizes/values need further targeted native probes. These implemented paths are not certified for every legacy form. Script-enabled custom XSL is a caller-selected legacy capability and executes with the caller's rights.

An earlier unsigned candidate SHA256 `4a349fa5611eb48daf6018846ccbfaef9f7495b17648908b083b24da63ed766c` was quarantined as `Trojan:Win32/Bearfoos.A!ml` (ThreatID 2147731250, successful protection action). Protection settings were left enabled and unchanged. The detection remains **unresolved**; later corpus execution or code signing does not clear it or establish a false positive. Its recorded details are in `docs/research/evidence/2026-10-15/defender-detection.json`. The measured baseline executables are unsigned. For a GitHub release, inspect its manifest, signature receipt, and Windows Authenticode status to establish signing status.

Packages contain only this project's executable, README, this report, manifest and checksums. They contain no original Microsoft executable, copied Microsoft stylesheet, credential or reference capture. The manual GitHub workflow additionally publishes signature receipts, archive checksums and release context as separate draft assets. Local packaging does not publish or install anything.
