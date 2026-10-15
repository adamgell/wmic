# WMIC compatibility research

Research date: 2026-10-15, America/New_York.

## Objective and evidence status

The requested outcome is a Rust Windows CLI that can replace classic
`wmic.exe` for existing callers. Full classic compatibility is the target,
delivered in stages. This file is a research inventory, not an assertion that
any replacement functionality has been implemented or verified.

Record each behavior as documented, observed on a particular reference build,
implemented, or verified against that build. Documentation alone does not
establish exact runtime behavior. Provider availability and permissions remain
properties of the Windows environment.

## Reference environment

- User-selected environment: the existing Parallels guest named
  `Windows 11 Home Domain`.
- Parallels guest UUID: `{ec3bc428-7a31-4f17-8424-7ce78923f7f3}`.
- Observed guest computer name: `DESKTOP-N4H9E7R`.
- Observed OS: Windows 11 Pro, ARM64, version/build `10.0.26200.8037`.
- Guest execution works through Parallels Tools as `NT AUTHORITY\SYSTEM`.
  This does not establish behavior under the user's normal account.
- Initial inspection found no `wmic.exe` on PATH or in the standard
  `System32\wbem` and `SysWOW64\wbem` locations.
- The `root\cli` repository is present. A read-only query returned 81
  `MSFT_CliAlias` instances with friendly names and target queries.
- Exported 81 full alias definitions and 10 `MSFT_Cli*` schema classes to
  [the evidence directory](evidence/2026-10-15/). These captures were taken
  while optional component installation was in progress, before validating
  an installed WMIC executable.
- The user has indicated that installation of the WMIC optional component may
  be needed. Component availability and installation are being checked.

## Command surface to enumerate

| Area | Required coverage |
| --- | --- |
| Entry points | One-shot invocation, interactive stdin, redirected stdin, batch files, shell pipelines, direct process invocation |
| Grammar | Case handling, whitespace, quoting, escaping, punctuation, abbreviations if supported, invalid and incomplete input |
| Targets | All reference aliases; custom aliases and roles; `PATH`; `CLASS`; explicit instance paths; `WHERE` expressions |
| Verbs | Default verb, `GET`, `LIST`, `ASSOC`, `CALL`, `CREATE`, `SET`, `DELETE` |
| Views | `BRIEF`, `FULL`, `INSTANCE`, `STATUS`, `SYSTEM`, `WRITEABLE`, and alias-specific views |
| Help | Global, command, alias, verb, property, method, parameter, and switch help |
| Session commands | `CONTEXT`, `QUIT`, `EXIT`, switch persistence and reset behavior |
| Failures | Parse errors, no matching instances, invalid properties/classes/namespaces, unavailable providers, denied access, connection failure, method failures |

The observed alias count is a fact about this guest, not a universal count for
all WMIC versions. Enumerate definitions and nested metadata rather than
assuming that an alias is only a class-name shortcut.

The exported metadata also documents positional `PWhere` substitutions and
contains derived verbs. For example, the `Alias` definition contains a
`SeeAlso` verb whose derivation invokes another WMIC command. Account for
these paths when investigating alias dispatch. The meaning of each observed
`VerbType` value must be established from behavior or authoritative schema
information before implementation.

## Switch surface

Documented global switches to measure:

`/NAMESPACE`, `/ROLE`, `/NODE`, `/IMPLEVEL`, `/AUTHLEVEL`, `/AUTHORITY`,
`/LOCALE`, `/PRIVILEGES`, `/TRACE`, `/RECORD`, `/INTERACTIVE`, `/FAILFAST`,
`/USER`, `/PASSWORD`, `/OUTPUT`, `/APPEND`, `/AGGREGATE`.

Documented verb switches and modifiers to measure:

`/VALUE`, `/ALL`, `/TRANSLATE`, `/EVERY`, `/FORMAT`, `/RESULTCLASS`,
`/RESULTROLE`, `/ASSOCCLASS`, and delete confirmation overrides.

Measure switch ordering, duplicate switches, empty values, reset semantics,
command-local versus persistent settings, and interactions. In particular,
the legacy documentation describes order-dependent `/FORMAT` and `/TRANSLATE`
behavior and special `/OUTPUT` versus `/APPEND` precedence.

## Output contract

Record stdout and stderr independently as bytes, together with the full Windows
process exit status. Record file and clipboard results separately from stdout.
Terminal output forwarded by Parallels is useful for discovery; it is not
sufficient evidence for the original WMIC byte stream.

Measure:

- Console, pipe, shell redirection, `/OUTPUT`, `/APPEND`, and `/RECORD` behavior.
- Encoding, code pages, BOMs, line endings, blank lines, trailing spaces,
  header capitalization, column ordering, widths, and instance ordering.
- Null and empty values; arrays; embedded objects; references; booleans;
  integer ranges; floating point; datetime and interval values; Unicode.
- Table and value-list output; CSV; XML/raw XML; MOF; HTML; built-in format
  names; explicit XSL paths and format parameters; translation tables.
- Prompts, localized messages, contextual help, error destinations, and
  output for an empty result set.
- The distinction between the process exit status, COM/WMI errors, and a
  provider method's returned value.

Compare stable queries against the same machine and account. For volatile
properties such as process lists and free memory, document the uncertainty;
do not normalize formatting differences away in byte-comparison tests.

## Reference capture record

Each eventual case should contain:

1. Case ID, purpose, and whether it can change guest state.
2. Original executable path, version, hash, signature status, and architecture.
3. Windows version/build, execution identity, locale, code pages, and relevant
   environment information.
4. Launch route and exact raw command line, plus stdin bytes where applicable.
5. Raw stdout/stderr bytes and full exit status; timeout and cancellation
   outcomes must remain distinguishable from process errors.
6. Output-file/clipboard effects where relevant and the expected instance data.
7. Repeatability notes and the evidence file locations.

Initial behavioral captures will use reviewed read-only operations. Do not
blindly execute every alias or method: some providers have side effects even
when queried. In particular, leave `Win32_Product` enumeration out of automatic
discovery. Write-operation tests should use explicit, isolated test objects on
the user-selected guest and preserve unrelated machine state.

## Questions that require measurements

- Which WMIC executable version can this guest supply after optional component
  installation, and which formats/resources accompany it?
- Does this version parse the raw Windows command line differently from the
  normal Rust/CRT argument vector?
- Which defaults and encodings change with the output destination or account?
- How do custom role namespaces and built-in alias metadata interact?
- Which authentication settings and remote error behaviors require a second
  authorized endpoint to verify?
- Which behaviors vary by Windows build, architecture, locale, or provider?

The current Microsoft reference and its linked legacy material disagree in
places. For example, the current page describes an ampersand prefix for a node
list file, while the legacy switch reference uses `@`. Association-output
defaults also differ between the descriptions. Preserve these as experiments
instead of selecting one interpretation without evidence.

## Primary sources reviewed

- [Current WMIC reference](https://learn.microsoft.com/en-us/windows/win32/wmisdk/wmic)
- [Windows command reference](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/wmic)
- [WMIC aliases](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc736307(v=ws.10))
- [WMIC switches](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc787035(v=ws.10))
- [WMIC verbs](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc784966(v=ws.10))
- [Creating and editing aliases](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc785304(v=ws.10))
- [Creating and editing formats](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc757287(v=ws.10))
- [IWbemLocator::ConnectServer](https://learn.microsoft.com/en-us/windows/win32/api/wbemcli/nf-wbemcli-iwbemlocator-connectserver)
- [IWbemServices::ExecMethod](https://learn.microsoft.com/en-us/windows/win32/api/wbemcli/nf-wbemcli-iwbemservices-execmethod)
- [CommandLineToArgvW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-commandlinetoargvw)
- [Microsoft Rust for Windows](https://github.com/microsoft/windows-rs)
