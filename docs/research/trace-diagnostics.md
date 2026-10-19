# Classic TRACE diagnostics

`/TRACE:ON` emits classic WMIC diagnostic records on stderr: `SUCCESS`/`FAIL`, six-column source-line padding, legacy source labels, optional `Result:  ` text and CRLF separators. These labels describe a compatibility protocol. They are reconstructed from command metadata and backend outcomes, not Rust source locations or a live COM log.

The earlier implementation emitted one request line. The replacement covers measured alias traversal, PATH validation, GET/LIST queries, actual enumeration counts, successful no-input method results, and table, VALUE, CSV and RAWXML formatting. Interactive commands retain resource state while tracing is off. Streamed methods place records before each existing result separator. Password values remain redacted, including a password repeated in a selector.

TRACE also adds the HRESULT and facility to errors and substitutes longer WMI descriptions from WMIC's resources. The implementation retains 109 observed HRESULT/resource mappings. Unmapped errors retain the backend description. The original attempts an absent resource for `0x8004105a`; the replacement uses the fallback.

## Tests and reference

The [23-case manifest](../../tests/fixtures/trace-cases.json) and [raw captures](../../tests/fixtures/trace-reference/) cover alias spelling, queries, multiple projections, PATH, empty results, invalid classes/properties/namespaces, formats, help, security settings, OFF, cold/warm interactive sessions, and one/two-result `GetOwner` calls. They perform no power or configuration operations. The existing RAWXML COMMAND STARTTIME policy applies only to that stdout attribute; stderr has no normalization.

`cargo test --test trace` compares complete records with controlled provider outcomes. Windows CI also launches the actual CLI for 17 stable commands and compares full process status and stderr. CPU instance counts and method paths are provider-dependent and are compared on the selected reference VM. CLI readers drain both pipes concurrently and terminate/reap timed-out children. Select `--suite trace` in `tools/reference/run_corpus.py` for differential captures; see the [testing guide](../testing.md).

Reference SHA256 values:

- ARM64 executable: `2ef794c4940afea56c6d6d3b9bc9e66965bc7333e79c5dcb2def3c0e36f0834c`.
- x86 executable: `4afe43ff64ad20475d75d9816a269fc03e10c492033b67e60c9886f3512e900e`.
- English MUI: `5e3bc78af6eb1aea40c6899c452ca8701d28756bf1eb5f056207fc963cfea17b`.

The executable's embedded version is 10.0.26100.5074. The MUI reports 10.0.26100.1, which runtime FileVersionInfo selects. Hashes identify the reference independently of those labels. This run staged pinned originals/MUI under ProgramData and restored missing Microsoft formatting data for testing. No original executable was installed in a system directory. Original executables, MUI files and stylesheets are excluded from the repository and packages.

## Static evidence

REA/Ghidra confirmed the ARM64 stderr emitter at `0x140035fd0` and x86 emitter at `0x433606`. They test the HRESULT sign, support an optional result line and suppress `COMMAND:` log events. The dated/threaded WMIC.LOG path is separate. Queries emit one initial enumerator record and a `GetText`/`Next` pair per object; interactive resource creation is cached.

The verbose wrapper at `0x140012fb0` uses the tool's resources after obtaining ordinary status text. The human formatter at `0x140020ea0` passes the TRACE setting to it. Retained evidence IDs include `ev_6eddc1b24b47610de2b6c64479cbedcdde2cf43dfbacbaa0b0e8c0b01efbc5d1` (emitter), `ev_4ac371a46cfc01d8b7460894a7603df390cbf5c187a2e72890470b5adca8c104` (query), `ev_3dc858a6928f2d36ab110ac457ecf8498356b0924476f2d7ce6021afe46c233a` (resource switch), and `ev_b019349e751e392df433f5ee8aa31f00909e70957e4d7103e88f219b8c99a027` (TRACE flag).

## Bounds

Mutation operations, failed/parameterized CALL traces, CLASS/schema traces, multiple remote-node traces, custom/chained/external formats and every interactive cache invalidation sequence remain unverified. English resources do not establish localized support. macOS shares the renderer for supported requests; Windows comparisons do not establish Microsoft WMIC behavior on macOS. Full TRACE or WMIC compatibility is not claimed.
