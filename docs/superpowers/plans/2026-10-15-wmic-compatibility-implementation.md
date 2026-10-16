# Native WMIC Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a native Rust `wmic.exe` that reproduces classic WMIC, with measured compatibility delivered in stages.

**Architecture:** A portable library parses UTF-16 commands, resolves aliases, and renders typed results. A Windows adapter owns native WMI, process entry, security, and output destinations. A separate reference toolchain captures and compares Microsoft's executable without becoming a runtime dependency.

**Tech Stack:** Rust 1.98.1, edition 2024; Microsoft's typed `windows` bindings; `serde`/`serde_json` for metadata and fixtures; Python 3 standard library and Windows PowerShell for development-only capture; `cargo-xwin` for MSVC cross-builds from macOS.

**Spec:** [Approved design](../specs/2026-10-15-wmic-compatibility-design.md). Read it alongside this plan. The user approved this implementation plan and native execution here on 2026-10-15 at 19:57 EDT.

## Global Constraints

- The user selected full classic compatibility, delivered in stages.
- Intermediate builds must identify their supported coverage.
- The application must not require PowerShell, .NET, the original WMIC executable, or restoration of the WMIC optional component at runtime.
- Produce MSVC builds for `x86_64-pc-windows-msvc`, `i686-pc-windows-msvc`, and `aarch64-pc-windows-msvc`.
- Initial runtime support proposal: Windows 10 version 1809 and newer and Windows Server 2019 and newer for x64/x86, and Windows 11 for ARM64.
- Successful cross-compilation alone is not runtime evidence.
- Guest execution currently uses `NT AUTHORITY\SYSTEM`; normal-user behavior requires separate evidence.
- Do not overwrite Windows system files as part of development or test setup.
- Do not persist real credentials in command captures, fixtures, or logs.
- Automatic discovery must not enumerate `Win32_Product` or invoke provider methods indiscriminately.
- Keep provider results, COM errors, and full 32-bit process exit status distinct.
- Follow each task's red/green cycle; commit the tested deliverable before the next task.

## Review Focus

1. Quoted WQL and object paths must survive the Windows raw command line without a lossy `argv` round trip; Task 2 tests both quoting and unpaired UTF-16.
2. Reversed property requests and empty result sets must preserve measured headers, order, spacing, BOMs, and exit status; Tasks 5 and 6 compare raw captures.
3. A machine without `root\cli` must still support stock aliases, while a missing explicitly selected custom role must fail; Task 3 pins both cases.
4. Provider errors may arrive during enumeration after some objects; Tasks 4 and 7 preserve partial results, target identity, and error ordering.
5. Method return values may indicate failure while COM invocation succeeds; Task 9 verifies independent method, diagnostic, and process status handling.

## File map and shared interfaces

Use one Cargo package named `wmic-compat`, library `wmic_compat`, and binary
`wmic`. Keep the portable modules available on macOS; place Windows-only
dependencies and implementations behind `cfg(windows)`. Start with version
`0.1.0-dev`; this is a development identifier, not a selected final release.

| Files | Responsibility |
| --- | --- |
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.gitignore` | Reproducible development/build configuration |
| `src/lib.rs`, `src/main.rs` | Public library modules and thin process entry |
| `src/model/{mod,wide,command,cim,error}.rs` | UTF-16 text, syntax model, typed WMI results, diagnostics |
| `src/parse/{mod,lex,command,switches}.rs` | Context-sensitive WMIC grammar |
| `src/alias/{mod,metadata,resolve}.rs`, `assets/aliases.json` | Alias definitions, substitutions, role lookup, stock fallback |
| `src/session/{mod,settings,execute,interactive}.rs` | Settings, orchestration, ordered nodes, interactive state |
| `src/backend/{mod,request}.rs` | Portable backend contract and resolved operations |
| `src/backend/windows/{mod,com,connect,schema,query,methods,write}.rs` | WMI ownership, security, queries, schema and changes |
| `src/render/{mod,table,value,csv,xml,mof,help,diagnostic}.rs` | Pure object-to-representation formatting |
| `src/platform/windows/{mod,entry,encoding,output,xslt,console}.rs` | Native command line, destination encoding/routing, XML/XSL and console APIs |
| `tools/reference/{capture.ps1,parallels.py,compare.py}` | Development capture transport and comparison |
| `tools/reference/tests/`, `tests/support/mod.rs` | Transport/comparison tests and shared fixture helpers |
| `tests/{parser,aliases,rendering,session,backend_contract,conformance}.rs` | Portable boundary and conformance tests |
| `tests/fixtures/{reference-cases.json,parser,objects,reference}/` | Reviewed invocations, syntax fixtures, object data, raw reference streams |
| `tools/windows/{run-conformance.ps1,stage-artifact.ps1,controlled-writes.ps1}` | Explicit Windows validation and isolated fixture operations |
| `docs/compatibility/{features.json,verified-profiles.md,known-mismatches.md}` | Honest implementation and verification coverage |
| `tools/release/package.py`, `README.md` | Local packaging, hashes, invocation and deployment guidance |

Create modules when their owning task needs them rather than adding empty
files in advance. The following names are the shared boundary contracts;
private implementation details stay with their owning module.

- `WideText(Vec<u16>)`: owned Windows text without lossy conversion.
- `EntryMode::{Process, Interactive}` and `ParsedCommand`: parsed command,
  ordered switches/modifiers, target, selector, verb and arguments.
- `CimValue`, `Property`, `ObjectSet`, `ClassSchema`, `MethodResult`,
  `OperationResult`, `ManagementResult`, `ManagementError`: portable typed data.
  `ManagementError` retains `code: u32`, `target: WideText`, extended
  diagnostics and `partial: Option<Box<ManagementResult>>` so an enumeration
  failure cannot discard objects already returned.
- `parse_command(&WideText, EntryMode) -> Result<ParsedCommand, ParseError>`.
  Process mode includes removal of the executable prefix from the raw line.
- `AliasStore::resolve(&ParsedCommand, &SessionSettings) -> Result<ResolvedCommand, ResolveError>`.
- `ResolvedCommand::{Management(ResolvedRequest), Derived(WideText), External(ExternalCommand), Help(HelpRequest), Context, Exit}`.
- `Management::execute(&mut self, &ResolvedRequest) -> Result<ManagementResult, ManagementError>`.
  Requests include effective connection settings, target/operation and typed
  parameters; the Windows backend owns connection/schema caches.
- `render(&RenderRequest, &ManagementResult) -> Result<RenderedOutput, RenderError>`.
  `RenderedOutput` contains separate `stdout: WideText`, `stderr: WideText` and
  `record: Option<WideText>` representations, before destination byte encoding.
- `OutputSink::write(&mut self, &OutputEvent) -> Result<(), OutputError>`;
  `OutputEvent` identifies stdout/stderr/record output and target.
- `Session::execute(&mut self, &ParsedCommand, &mut dyn Management, &mut dyn OutputSink) -> CommandOutcome`.
  `CommandOutcome` retains `exit_status: u32`, operation data and diagnostics.

## Task 1: reference capture and feature inventory — Stage 0

**Files:** Create `tools/reference/`, its tests, `tests/fixtures/reference-cases.json`, `tests/fixtures/reference/`, and `docs/compatibility/features.json`.

**Interfaces:** `capture.ps1` accepts a case manifest and explicit executable
path; returns JSON with UTF-16 command line, base64 streams, `exit_status`, and
`completed|timeout|cancelled`. `parallels.py capture --vm ID --exe PATH
--manifest FILE --out DIRECTORY` transports through `prlctl exec` using stdin,
not an oversized `-EncodedCommand`. `compare.py EXPECTED ACTUAL` exits zero only
when the selected fields/bytes match.

- [x] Add transport/comparison tests that assert preservation of `b'\xff\xfeA\x00\r\x00\n\x00'`, independent stderr, exit `0x80041017`, and a timeout distinct from exit zero.

  ```python
  self.assertEqual(base64.b64decode(result['stdout_base64']), b'\xff\xfeA\x00\r\x00\n\x00')
  self.assertEqual(result['exit_status'], 0x80041017)
  self.assertEqual(timeout_result['state'], 'timeout')
  ```

- [x] Run `python3 -m unittest discover -s tools/reference/tests -v`; confirm these tests fail before implementation.
- [x] Implement capture using redirected native process byte streams with concurrent stdout/stderr draining. Pin executable identity and refuse a replacement executable as the oracle. Record account and launch context; close unused stdin for one-shot cases.
- [x] Seed the corpus from the 23 preliminary pipe cases and installed profile under `docs/research/evidence/2026-10-15/`. Capture global/contextual help, stable OS queries, reversed properties, table/value/CSV/XML/MOF modes, case variants, empty results, and parse/provider errors with the reproducible tool. Use explicit original executable paths. Verify alias metadata after installation and link the captured profile to the existing evidence.
- [x] Run the Python tests and compare two repeated stable captures. Expected: identical selected bytes/status, with volatile environment fields reported separately. Commit as `test: add byte-preserving WMIC reference captures`.

## Task 2: portable data model and WMIC parser — Stage 1

**Files:** Create Cargo configuration, `src/lib.rs`, `src/model/`, `src/parse/`, `tests/parser.rs`, `tests/support/mod.rs`, and parser fixtures.

**Interfaces:** Produce `WideText`, the command/error/CIM model, `EntryMode`,
`parse_command`, and fixture helper `wide(&str) -> WideText`. Parsed selectors
are `Option<WideText>`. Preserve ordered modifiers and raw spans;
keyword case folding must not rewrite user values.

- [x] Add `parser_preserves_wql_quotes`, `parser_preserves_unpaired_utf16`, `parser_distinguishes_reset_from_omitted`, and `parser_does_not_split_quoted_commas`. Assert the exact UTF-16 units, selector, properties and switch order against manually reviewed fixtures derived from Task 1.

  ```rust
  assert_eq!(parsed.selector.as_ref().unwrap(), &wide("Version like '10.%'"));
  assert_eq!(round_tripped_unpaired.0, vec![0xD800]);
  assert_eq!(parsed.modifiers, expected_ordered_modifiers);
  ```

- [x] Run `cargo test --test parser`; confirm the new cases fail before implementing their behavior.
- [x] Implement the lexer/parser and model contracts above. Pin Rust `1.98.1`; enable Windows APIs only for Windows targets. Keep `serde`/`serde_json` metadata separate from raw UTF-16 fixture fields. Add no WMI-dependent parsing.
- [x] Run `cargo test --test parser` and `cargo fmt --check`; expected: all parser cases pass without Windows or PowerShell. Commit as `feat: parse classic WMIC commands without losing UTF-16 syntax`.

## Task 3: alias metadata, views, substitutions and fallback — Stage 1

**Files:** Create `src/alias/`, `assets/aliases.json`, and `tests/aliases.rs`; extend model/fixtures only for alias contracts.

**Interfaces:** Produce `AliasStore`, `ResolvedCommand`, and `ResolveError`.
Alias entries retain connection defaults, target, `PWhere`, formats, derived
properties, verbs, parameters and qualifiers. Stock definitions operate without
the original installation; live metadata is supplied through a testable loader.

- [x] Add `stock_aliases_work_without_root_cli`, `missing_explicit_role_does_not_fallback`, `alias_positional_selector_uses_pwhere`, and `alias_seealso_expands_to_derived_command`. Assert exact resolved targets and UTF-16 substitutions from the captured alias definitions.

  ```rust
  assert_eq!(effective_class, wide("Win32_OperatingSystem"));
  assert!(missing_custom_role.is_err());
  assert_eq!(expanded_command, expected_seealso_command);
  ```

- [x] Run `cargo test --test aliases`; verify the new behavior fails before implementation.
- [x] Implement resolution, metadata validation, packaged stock definitions and measured precedence rules. Determine and test observed `VerbType` semantics before dispatching them. Include all 81 selected-profile stock aliases; do not make runtime access query those providers during startup.
- [x] Run `cargo test --test aliases`; expected: the selected alias corpus resolves and absent/custom-role behavior matches its fixtures. Commit as `feat: resolve WMIC aliases and stock metadata independently`.

## Task 4: native local connection, schema and query adapter — Stage 1

**Files:** Create `src/backend/`, `src/backend/windows/{mod,com,connect,schema,query}.rs`, `tests/backend_contract.rs`, and Windows backend fixtures.

**Interfaces:** Produce `Management::execute`, `ResolvedRequest`, and
`WindowsManagement::new() -> Result<WindowsManagement, ManagementError>`.
The request operation initially supports queries and class/instance retrieval;
later operations must return a tracked unsupported result until implemented.

- [ ] Add typed adapter cases for null versus empty string/array, unsigned 64-bit values, embedded objects, and errors after partial enumeration. Assert retained types, objects, target and original WMI code. Pin the preliminary missing direct-path class status to `0x80041002`; do not substitute a different raw `ExecQuery` error.

  ```rust
  assert_eq!(missing_class.code, 0x80041002);
  assert!(enumeration_error.partial.is_some());
  assert_ne!(null_value, empty_string_value);
  ```

- [ ] Run `cargo test --test backend_contract`; confirm unimplemented conversions fail. Add Windows-only ignored live tests `local_os_query` and `invalid_class_preserves_hresult`.
- [ ] Implement RAII COM initialization, process security, native WMI connection/schema/query calls and typed conversion. Localize unsafe code and document apartment/interface ownership. Preserve extended errors and partial enumeration outcomes in the result model.
- [ ] Run portable adapter tests; cross-build with `PATH="/opt/homebrew/opt/llvm/bin:$PATH" cargo xwin build --target aarch64-pc-windows-msvc`. Run the explicit live tests on the selected guest through the development transport. Expected: real OS data and the reference invalid-class error. Commit as `feat: query local WMI through native COM`.

## Task 5: table, value and diagnostic rendering — Stage 1

**Files:** Create `src/render/{mod,table,value,diagnostic}.rs`, `src/platform/windows/encoding.rs`, `tests/rendering.rs`, and typed object fixtures.

**Interfaces:** Produce `RenderRequest`, `RenderedOutput`, `render`, and
`encode_output(&WideText, &DestinationProfile) -> Result<Vec<u8>, OutputError>`.
`DestinationProfile` describes console/pipe/file encoding measured in Task 1.

- [ ] Add golden cases `reversed_get_properties_match_reference`, `empty_query_matches_reference`, `array_null_and_boolean_rendering`, and `invalid_property_diagnostic_matches_reference`. Assert encoded bytes against the corresponding reference capture, not trimmed text. The preliminary pipe fixtures include `CR CR LF` and an empty query that writes stderr with status `0`; preserve both.

  ```rust
  assert_eq!(table_bytes.as_slice(), include_bytes!("../docs/research/evidence/2026-10-15/pipe-cases/os-get.stdout.bin"));
  assert_eq!(empty_stdout.as_slice(), b"\r\r\n\r\r\n");
  assert_eq!(empty_stderr.as_slice(), b"No Instance(s) Available.\r\r\n");
  ```

- [ ] Run `cargo test --test rendering`; confirm byte comparisons fail before implementing the formats.
- [ ] Implement measured default views, property ordering, alignment, blank lines, value syntax, diagnostic destinations and destination encoding. Keep renderer input typed and deterministic.
- [ ] Run `cargo test --test rendering`; expected: byte equality for the stage's corpus. Commit as `feat: reproduce WMIC table and value output`.

## Task 6: one-shot executable and full query-path conformance — Stage 1

**Files:** Create `src/main.rs`, `src/session/{mod,settings,execute}.rs`, `src/platform/windows/{mod,entry,output}.rs`, `tests/session.rs`, `tests/conformance.rs`, and `tools/windows/{stage-artifact,run-conformance}.ps1`.

**Interfaces:** Produce `SessionSettings`, `Session::execute`, `OutputSink`,
`CommandOutcome`, and the Windows entry point using the raw command line.
Staged artifacts live under `C:\ProgramData\WmicCompat\artifacts\<SHA256>\`.
The staging tool verifies the file hash and never overwrites the original WMIC.

- [ ] Add `unknown_switch_is_not_ignored`, `empty_result_keeps_reference_status`, `quoted_wql_reaches_backend_unchanged`, and `stderr_is_not_merged_into_stdout` using a recording backend/sink. Assert resolved operations, bytes and `u32` process status separately.

  ```rust
  assert_eq!(unknown_switch.exit_status, 0xAC5D);
  assert_eq!(empty_query.exit_status, 0);
  assert_eq!(captured_stderr.as_slice(), b"Invalid Global Switch.\r\r\n");
  ```

- [ ] Run `cargo test --test session --test conformance`; verify the new execution cases fail before orchestration is implemented.
- [ ] Wire parser, resolver, backend and renderer. Use the correct Windows API for the full 32-bit exit code. Provide an explicit unsupported-platform result on non-Windows while keeping portable tests available.
- [ ] Cross-build ARM64, stage only the test-owned executable, and run Task 1's query cases against both executables. Run real `cmd.exe`/batch and PowerShell cases as well as direct launch. Expected: stage-case parity with all mismatches reported. Commit the working partial CLI as `feat: run native WMIC-compatible local query commands`.

## Task 7: remote settings and ordered multi-node execution — Stage 2

**Files:** Extend `connect.rs`, session settings/execution and request types; add `tests/fixtures/connections/`, connection/session tests, and explicit remote cases.

**Interfaces:** Extend `ConnectionOptions` with node, namespace, identity,
authority, authentication/impersonation, locale and privilege settings. Keep
secrets out of serialized diagnostics. Produce `load_nodes(&WideText) -> Result<Vec<WideText>, InputError>` and ordered per-node outcomes.

- [ ] Add tests for node-list files, switch resets, authentication/authority conflicts, per-interface security application, and first-node success followed by a second-node enumeration failure. Assert preserved order, partial output and redacted secrets.

  ```rust
  assert_eq!(visited_targets, vec![wide("node-a"), wide("node-b")]);
  assert_eq!(second_node_error.target, wide("node-b"));
  assert!(!persisted_trace.contains("test-only-secret"));
  ```

- [ ] Run the relevant session/backend tests; confirm new connection cases fail first.
- [ ] Implement measured remote/security switches, credential prompts, proxy configuration, node-list syntax, trace output, fail-fast checks and aggregation. Preserve locale/privilege settings and supply the repeated-query loop needed by `/EVERY`; Task 12 adds its interactive and cancellation behavior. Set no invented timeout. Use only explicitly selected remote endpoints for live tests.
- [ ] Run portable tests and authorized live remote cases. If no second endpoint is authorized, record remote implementation as unverified and leave the live acceptance item open. Commit as `feat: support WMIC connection settings and ordered node execution`.

## Task 8: association, direct schema and contextual help completion — Stage 2

**Files:** Extend `schema.rs`, `query.rs`, alias resolution and parsing; create `src/render/help.rs` and schema/help fixtures.

**Interfaces:** Add association requests with result class/role/association
filters, direct class operations, and `HelpRequest` schema/alias/verb contexts.

- [ ] Add reference cases for `PATH`, `CLASS`, `ASSOC`, filters, writable/system views and contextual property/method help. Assert exact selected schema and rendered help bytes; distinguish absent provider metadata from an empty result.

  ```rust
  assert_eq!(get_help.as_slice(), include_bytes!("../docs/research/evidence/2026-10-15/pipe-cases/os-get-help.stdout.bin"));
  assert_eq!(resolved_association_filters, expected_association_filters);
  ```

- [ ] Run the matching parser/alias/rendering/backend tests and confirm new behavior fails first.
- [ ] Implement the corresponding native calls and metadata-driven help. Validate extra discovered command names against reference behavior rather than enabling names found only in legacy metadata.
- [ ] Run the selected corpus against the ARM64 guest. Expected: associations, class/schema behavior and help match the pinned profile. Commit as `feat: complete WMIC schema traversal and contextual help`.

## Task 9: schema-driven methods, writes and derived verbs — Stage 3

**Files:** Create `methods.rs`, `write.rs`, `tools/windows/controlled-writes.ps1`; extend aliases, parser, session and typed results; add method/write fixtures.

**Interfaces:** Add typed method/create/update/delete requests, `MethodResult`
with independent return/out parameters, and supported derived/external command
dispatch. Controlled fixtures return their exact owned identifiers for cleanup.

- [ ] Add `method_return_failure_is_not_com_failure`, positional/named/optional argument conversion cases, static versus instance dispatch, confirmation behavior, and a recorded test-owned process/environment-variable lifecycle. Assert target IDs before each change and cleanup after failure.

  ```rust
  assert!(synthetic_com_invocation.is_ok());
  assert_eq!(synthetic_provider_return_code, 5);
  assert_eq!(changed_object_id, owned_fixture_id);
  ```

- [ ] Run the relevant portable tests; confirm missing conversions and dispatch fail first. Keep live write cases explicitly selected and separate from default discovery.
- [ ] Implement schema-driven native methods and writes, derived command expansion and required class operations. Never invoke every provider method as a discovery strategy.
- [ ] Run controlled tests on the selected guest, operating only on recorded owned objects. Expected: reference effects/output/status match, cleanup succeeds, and unrelated state is untouched. Record unexercised destructive methods. Commit as `feat: execute WMIC methods and controlled instance changes`.

## Task 10: CSV, XML, MOF, HTML, XSL and translation — Stage 4

**Files:** Create remaining renderer modules, `src/platform/windows/xslt.rs`, format definitions, and format/translation fixtures.

**Interfaces:** Produce `to_wmi_xml(&ManagementResult) -> Result<WideText, RenderError>` and `transform(&WideText, &FormatSpec) -> Result<RenderedOutput, RenderError>`. `FormatSpec` retains ordered parameters and translation position.

- [ ] Add golden CSV escaping/Node-column cases, XML null/array/reference cases, MOF/HTML cases, parameterized local XSL, and reversed format/translation order. Assert exact bytes and measured engine behavior.

  ```rust
  assert_eq!(csv_bytes, oracle_csv_bytes);
  assert_eq!(parameterized_xsl_bytes, oracle_parameterized_xsl_bytes);
  assert_eq!(format_then_translate, oracle_format_then_translate);
  ```

- [ ] Run renderer tests and confirm new formats fail before implementation.
- [ ] Implement canonical XML and built-in formats. Use the native XML/XSL engine identified by reference dependency/behavior inspection for custom transforms; record engine identity in the reference profile. Do not depend on WMIC's installed XSL files.
- [ ] Run portable golden tests and Windows transform cases. Expected: all selected formats/parameters/translations match. Commit as `feat: reproduce WMIC structured and custom output formats`.

## Task 11: output destinations, recording and encoding interactions — Stage 4

**Files:** Extend `output.rs`, `encoding.rs`, session routing and Windows tests; add destination fixtures.

**Interfaces:** Implement stdout/stderr, overwrite/append files, XML recording
and clipboard sinks while preserving `OutputEvent` ordering. Resolve routing
from explicit settings and measured precedence.

- [ ] Add tests for conflicting output/append destinations, new and existing files, BOM/encoding differences, write failures, and records separate from ordinary output. Assert exact bytes and preserved pre-existing unrelated file content.

  ```rust
  assert_eq!(written_file_bytes, oracle_output_file_bytes);
  assert_eq!(appended_file_bytes, oracle_appended_file_bytes);
  assert_eq!(unrelated_file_after, unrelated_file_before);
  ```

- [ ] Run sink/routing tests; confirm new destination behavior fails first.
- [ ] Implement native routing and clipboard APIs. Clipboard live cases must be explicitly selected and preserve/restore existing clipboard contents; if restoration cannot be guaranteed, record the case as requiring manual validation rather than clobbering it.
- [ ] Run file cases only under an owned test directory and compare raw files/status. Run selected clipboard cases under the stated constraint. Commit as `feat: match WMIC output routing recording and encodings`.

## Task 12: interactive session, repetition and cancellation — Stage 4

**Files:** Create `interactive.rs`, `console.rs`, interactive transcript fixtures and tests; extend session/controller behavior.

**Interfaces:** Produce `run_interactive(&mut Session, &mut dyn Management, &mut dyn InputSource, &mut dyn OutputSink) -> CommandOutcome`. `InputSource::next_input(&mut self) -> Result<InputEvent, InputError>` returns a UTF-16 line, EOF, or interruption and retains whether its source is a console or redirected input.

- [ ] Add transcripts for startup/prompt, context changes and reset, routing scope, help, confirmation, EOF, `QUIT`/`EXIT`, `/EVERY`, and cancellation between nodes/repetitions. Assert transcript bytes and state before each command.

  ```rust
  assert_eq!(transcript_bytes, oracle_transcript_bytes);
  assert_eq!(namespace_after_reset, wide("root\\cimv2"));
  assert_eq!(interactive_outcome.exit_status, oracle_exit_status);
  ```

- [ ] Run session/transcript tests; confirm new behaviors fail first.
- [ ] Implement interactive control and measured console behavior without introducing extra ANSI UI or project banners. Keep `/INTERACTIVE` confirmations distinct from shell mode.
- [ ] Run redirected-input and actual console cases on Windows. Expected: corresponding reference transcripts and status match; pipe tests are not reported as console verification. Commit as `feat: reproduce classic WMIC interactive sessions`.

## Task 13: platform matrix, unchanged scripts and local packages — Stage 5

**Files:** Create `tools/release/package.py`, packaging tests and `README.md`; finalize compatibility reports and explicit platform case manifests.

**Interfaces:** `package.py --target TRIPLE --binary PATH --out DIRECTORY`
produces a local archive and SHA-256 manifest. The report links each claim to
its reference profile and case IDs; no upload or publication occurs implicitly.

- [ ] Add package tests asserting Windows filename `wmic.exe`, archive/hash integrity, and exclusion of credentials/original Microsoft executable. Add unchanged-script cases that exercise parsing, piping, error handling and hardcoded-path behavior.

  ```python
  self.assertIn('wmic.exe', archive.namelist())
  self.assertEqual(archive.read('wmic.exe'), supplied_build_bytes)
  self.assertEqual(executable_sha256, manifest['wmic.exe']['sha256'])
  ```

- [ ] Run package tests and confirm packaging assertions fail first.
- [ ] Implement local packages and deployment guidance, including explicit-path comparisons, PATH precedence and legacy hardcoded locations. Do not overwrite system binaries. Populate verified/implemented/unverified/mismatched coverage from actual results.
- [ ] Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, Python tool tests, MSVC cross-builds for all three targets, and `cargo xwin clippy --all-targets --target aarch64-pc-windows-msvc -- -D warnings` with the LLVM path configured. Install the missing i686 standard-library target when implementation begins; use `XWIN_ARCH=x86,x86_64,aarch64` for its cross-build dependencies.
- [ ] Run the full corpus and unchanged scripts on each authorized runtime/profile. Keep unavailable platforms open as unverified. Obtain the selected execution workflow's whole-branch review, resolve actionable findings, rerun affected checks, and commit as `build: package and document verified WMIC compatibility`.

## Execution and stop conditions

Recommend **Native execution in this chat**: the early tasks share parser,
metadata and raw-output contracts, so a continuous implementation context is
useful. Follow `superpowers:executing-plans` task-by-task and use its required
review process. If the user instead selects subagent-driven execution, follow
that skill and delegate tasks only after that selection.

After each stage, report working commands, verification evidence and remaining
coverage. Do not stop the full project merely because Stage 1 produces a useful
binary. Keep approval/endpoint-dependent live tests open and continue independent
authorized tasks. Do not fabricate results or substitute cross-compilation for
runtime acceptance.

The user reviewed this concrete plan and selected native execution in this
chat. Continue task-by-task without requesting approval between tasks. Preserve
the separate boundaries for external publication, additional live endpoints,
and changes to unrelated machine state.
