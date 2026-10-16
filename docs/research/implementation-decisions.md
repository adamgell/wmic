# Implementation decisions

Decisions recorded during the approved 13-task native implementation. Later decisions explicitly supersede earlier ones where stated. Each preserves its reason and compatibility cost.

1. Ruling: Work in the current clean codex/wmic-compatibility-spec checkout because the user explicitly selected execution here and this branch contains only this project's research/specification — do not request another environment approval — cost if wrong: changes could mix if another process edits this checkout; inspect status before each commit.

2. Task 2: Ruling: Define portable SessionSettings and ResolvedRequest data contracts during Task 2 alongside the shared model, before alias resolution consumes them — later tasks implement behavior — cost if wrong: early contracts may need compatible field extensions, which tests must expose.

3. Task 1: Ruling: Normalize only the explicitly opted-in raw XML root COMMAND STARTTIME attribute during comparison, retaining original raw bytes and integrity hashes — the original generates a wall-clock timestamp even for stable OS data — cost if wrong: this one metadata field's value is not compared for equality; all other XML and stream bytes remain strict. Added a failing normalization test and a guard proving property values are not normalized.

4. Task 2: Ruling: Use hand-checked literal parser fixtures in tests/parser.rs alongside captured syntax invocations rather than duplicate internal AST JSON snapshots — avoids tests that only detect representation changes — cost if wrong: new syntax expectations must be added to Rust tests as the corpus grows.

5. Task 3: Ruling: Derived/external requests carry their original resolved selection as well as the command template — per-instance #__PATH expansion cannot be correct from a template alone — cost if wrong: controller dispatch must honor this additional context.

6. Task 4: Ruling: Link the Windows C runtime statically for all three planned MSVC targets now — the first native test binary failed to load with 0xC0000135 because the ARM64 VC runtime DLL is absent — cost if wrong: larger executables and CRT fixes require rebuilding.

7. Task 6: Ruling: Evaluate Task 6 parity on its basic local query/diagnostic cases, and preserve all 23-case differences in stage1-conformance.json — the plan assigns contextual help to Task 8, structured formats to Task 10, and CONTEXT to session work — cost if wrong: this early executable is explicitly partial and cannot be called a full drop-in yet.

8. Task 7: Ruling: Finalize legacy /TRACE byte parity with Task 10's native XML/XSL stages, retaining the visible mismatch until then — its transcript includes formatter calls that do not exist before Task 10 — cost if wrong: trace compatibility remains incomplete in this intermediate commit.

9. Task 7: Ruling: Add opt-in bounded replay for only known Parallels lost-job-result errors — two transient PrlJob Invalid argument failures occurred with a healthy guest — cost if wrong: an explicitly marked replay-safe script could execute twice. Default scripts and guest application errors are never replayed; hash-verified staging is idempotent. Three new behavioral transport tests prove these boundaries.

10. Task 8: Ruling: Bring the class MOF-style HTML renderer forward from Task 10 to Task 8 — native CLASS GET emits this representation and schema acceptance depends on it — cost if wrong: Task 10 must reuse and extend this renderer rather than introduce conflicting output rules.

11. Task 8: Ruling: Keep volatile OS default probes as raw mismatch evidence and accept their fixed headers/status plus portable deterministic value tests — memory/time/process values change between the original and candidate runs — cost if wrong: these two live rows do not have an exact byte-parity claim; stable schema and help still require strict parity.

12. Task 9: Ruling: Add redirected-input support to the native reference launcher during Task 9 rather than Task 12 — confirmations require real N/Y/? input to prove mutation boundaries — cost if wrong: Task 12 must reuse this concurrent input/output launcher rather than introduce another stdin path. Default capture still uses EOF, read-only manifest guards and mutation replay prohibitions remain unchanged.

13. Task 9: Ruling: For the controlled process lifecycle, compare every byte except substitutions of the exact recorded owned PID in the Create output field, instance path, and matching selector — sequential reference/candidate processes necessarily receive different PIDs — cost if wrong: PID numeric equality is excluded for these explicitly owned fields; identity/CreationDate/CommandLine guards and zero-remnant checks still bind each action, raw streams are retained, and unrelated numeric/property changes fail comparison tests.

14. Task 10: Ruling: Redact global password values in presentation/record command-line metadata — the approved specification forbids credential disclosure through logs — cost if wrong: raw XML/record metadata with an explicitly supplied password intentionally cannot reproduce legacy plaintext credential bytes. Internal native authentication still receives SecretWide; selected reference captures prohibit passwords.

15. Task 10: Ruling: Extend opt-in timestamp normalization to the fixed HTML representation of the root COMMAND STARTTIME attribute only, anchored to its exact metadata prefix — XML view renders the same volatile wall-clock field as rawxml — cost if wrong: this metadata timestamp value is excluded from equality; properties, nested content, all other bytes and raw integrity hashes remain checked. Behavioral test observed RED then GREEN.

16. Task 10: Ruling: Preserve an implementation-specific /TRACE diagnostic rather than fabricate Microsoft C++ source locations and internal calls — this Rust backend uses different API sequences, so the measured legacy transcript cannot truthfully describe execution — cost if wrong: scripts comparing legacy trace text are incompatible; retain the mismatch in the release matrix and never claim full classic compatibility. This supersedes Task7 trace byte-parity scheduling.

17. Task 11: Ruling: Exclude only the anchored root WMIRECORD/first RECORD STARTTIME metadata from owned-record comparison, preserving raw files and checking provider CDATA and all remaining bytes — its timestamp changes between launches — cost if wrong: equality omits that clock value; a guard test rejects nested/property-content masking.

18. Task 12: Ruling: Extend record-clock comparison to all direct RECORD children of the WMIRECORD root, locating exact opening-tag byte spans with Expat and preserving every other byte rather than reserializing XML — persistent shell records contain multiple independently timed commands — cost if wrong: only these metadata clock values are excluded; CDATA and nested lookalikes remain strict. New multi-record guard observed RED→GREEN.

19. Task 12: Ruling: Reproduce the measured consumption of one pending console key after a management query — the reference consumes the q from a prequeued quit line, while ordinary line-at-a-time input is unaffected — cost if wrong: console typeahead may lose a character, as on the measured legacy profile; directed console regression retains this behavior explicitly.

20. Task 13: Ruling: Use bundled default-role metadata for read-only local ALIAS introspection when root\cli is absent, including an explicitly selected default role — the approved replacement must retain its alias catalog without the legacy namespace — cost if wrong: this virtual provider is immutable and its advanced/system-property representation remains unverified on a genuinely missing-namespace profile; custom absent roles still error. The captured provider enumeration order is data, kept separately from the sorted catalog.

21. Task 13: Ruling: Compare the x64 executable under ARM64 emulation against the installed ARM64 reference, labeling it as a cross-architecture comparison; compare x86 against the installed x86 reference — no x64 Microsoft executable/native x64 Windows host is available — cost if wrong: x64-specific legacy differences and native x64 hardware behavior remain unverified.
