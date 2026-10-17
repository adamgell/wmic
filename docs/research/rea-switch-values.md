# Legacy switch-value behavior recovered with REA

The 2026-10-16 investigation used REA 6.5.0 with Ghidra 12.1.4 on macOS ARM64, then corroborated the results with the original executables on the existing Windows ARM64 reference machine. The fixes cover numeric `/FAILFAST` conversion and unchanged `/LOCALE` assignments. Microsoft's internal `/TRACE` transcript remains an implementation-specific difference.

## Artifact identity and static observations

| Reference | Version | SHA-256 |
| --- | --- | --- |
| Microsoft ARM64 WMIC | 10.0.26100.1 | `2ef794c4940afea56c6d6d3b9bc9e66965bc7333e79c5dcb2def3c0e36f0834c` |
| Microsoft x86 WMIC | 10.0.26100.1 | `4afe43ff64ad20475d75d9816a269fc03e10c492033b67e60c9886f3512e900e` |

Both source Authenticode signatures were valid Microsoft Windows signatures. The original files stayed local and were not redistributed. REA imported private copies without executing the target; hashes were unchanged after analysis.

In the ARM64 binary, the switch dispatcher at `0x14002d7a8` calls `msvcrt!_wtol` for a non-ON/OFF FAILFAST value, rejects a signed result below 1, and stores the accepted DWORD at enclosing settings offset `0x2b4`. ON/OFF updates the enable flag without replacing the saved timeout. Relevant REA pseudocode Evidence ID: `ev_699f4a538d7c5aed2092ceaf0f2f9d4c8ccbf14941b81c6fcd22dd72e8c566ca`.

The locale setter at `0x140028a98` compares the new value with the stored locale ignoring case and returns without assigning when they compare equal. It also performs this comparison when resetting to the system UI-language default. Relevant Evidence ID: `ev_95d49a5521252da3a7aa0f093e9e1fe774216ad30d245dcfe93dbb0597d02f2f`.

These are observations of Ghidra's recovered code. Names/types and function boundaries may be incomplete; pseudocode is not original source. The behavior below was separately measured with the executables.

## Confirmed behavior

| Input | Original behavior | Earlier Rust behavior |
| --- | --- | --- |
| `/failfast:125ms context` | Accepts a timeout of 125 | Rejects with `0xAC68` |
| `/failfast:" 125" context` | Accepts leading legacy whitespace | Rejects with `0xAC68` |
| `/failfast:"１２５" context` | Accepts fullwidth decimal digits | Rejects with `0xAC68` |
| `/failfast:2147483648 context` | Signed result is negative; rejects | Accepts the unsigned value |
| `/failfast:-4294967295 context` | Wraps to 1; accepts | Rejects with `0xAC68` |
| `/locale:MS_409 context` on the selected default profile | Keeps `ms_409` | Replaces the stored spelling with `MS_409` |

The first 62 executable comparisons had 25 mismatches. The full test set now contains 286 distinct read-only CONTEXT invocations. All cases completed on both original ARM64 and x86 WMIC, with equal invocation, status and stream bytes across those references.

A direct, execution-only probe of the selected ARM64 `msvcrt.dll` (`7.0.26100.7705`) established numeric results and enumerated all nonzero BMP code units for decimal-digit and leading-whitespace behavior. The retained oracle includes 53 value conversions, 18 ten-digit ranges and 25 whitespace code units. The implementation preserves raw UTF-16 and explicit wrapping arithmetic, rather than relying on host Rust Unicode tables or a platform C `long` width. The distinction matters for characters such as U+180E and unsupported modern digit blocks. See [legacy-wtol.json](../../tests/fixtures/legacy-wtol.json).

Locale assignment now preserves the stored spelling for an equal value, including repeated assignments and empty resets. A changed locale retains the caller's spelling. The tests cover changing away from the default, returning to it with a different spelling, and resetting again.

## Automated regression coverage

- [switch_values.py](../../tools/reference/switch_values.py) reproducibly generates [286 cases](../../tests/fixtures/switch-values-cases.json) from the captured conversion oracle and explicit boundary/locale probes. They only request CONTEXT; they do not ping a host or invoke a management provider.
- [The reference capture](../../tests/fixtures/switch-values-reference/capture.json) retains raw status, UTF-16 invocation, profile and stream hashes. Identical byte streams share one content-addressed file; 16 files cover all 286 cases. This deduplication does not normalize output.
- [Portable controller tests](../../tests/switch_values_reference.rs) compare exact stdout, stderr and full DWORD status for every case. A backend that panics on use proves these probes never reach a management provider.
- The same test file launches the actual executable on Windows using `raw_arg`, null stdin and a bounded wait. Only the child process's `COMPUTERNAME` is set to the recorded reference label, making CONTEXT deterministic without altering the host. Existing Windows CI runs this test through ordinary `cargo test`.
- `WMIC_TEST_FIXTURE_DIR` and `WMIC_TEST_CLI` allow the compiled Windows test to run with explicitly staged files in the reference VM. Both test functions passed there, including all 286 actual CLI launches.
- The standard corpus and fixture-integrity gates include the new `switch-values` suite; missing cases, invalid states, argument changes and stream corruption still fail the gate.

The baseline macOS suite also exposed an unrelated projection mistake in a test: it requested only DeviceID from network disks and then read ProviderName. The test now requests both properties. A mounted network volume therefore exercises the assertion instead of causing a missing-property panic.

## Measured validation and limits

All three local Windows release builds match 286/286 cases with strict gate status 0. ARM64 runs natively against the ARM64 original; x64 runs under emulation against the ARM64 original, explicitly a cross-architecture comparison; x86 runs under emulation against the x86 original. [Committed validation reports](evidence/2026-10-16/rea-switch-values/summary.json) identify the candidate hashes and reference profiles.

The Mac suite passes 357 Rust tests and 53 Python reference tests. The Windows ARM64 test executable passes both controller and actual-CLI tests. Independent review verified the fixtures against both raw reference captures and found no actionable issue.

These results apply to this focused suite and selected Windows/CRT profile. They do not re-measure the entire earlier corpus, establish other Windows versions/locales or native AMD64/x86 hardware behavior, or close the existing TRACE/volatile-OS-data differences. CONTEXT acceptance does not itself measure elapsed ICMP timeout; the saved timeout values are supported by the direct CRT measurements and recovered setter data flow.

To repeat the focused comparison with a freshly built executable:

```sh
python3 tools/reference/run_corpus.py \
  --vm '<reference-vm-id>' \
  --target aarch64-pc-windows-msvc \
  --binary '<absolute-path-to-wmic.exe>' \
  --suite switch-values \
  --out '<fresh-output-directory>'
```

Native reference acquisition requires Microsoft's executable with the pinned SHA-256. Ordinary CI uses the committed byte oracles and does not download Microsoft binaries or require REA/Ghidra.
