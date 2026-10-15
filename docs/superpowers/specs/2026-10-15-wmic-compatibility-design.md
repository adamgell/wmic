# Native Rust WMIC replacement

Date: 2026-10-15, America/New_York.

Status: proposed written specification, following approval of the architecture.
Implementation has not started. Written-specification review remains required
before the implementation plan is prepared.

## Intended outcome

Build a native Rust Windows executable named `wmic.exe` that existing scripts
and applications can invoke in place of Microsoft's classic WMIC. The user
explicitly selected full classic compatibility, delivered in stages: queries,
aliases, remote connections, methods, formatting, and interactive mode.

Success means preserving observable command behavior: accepted syntax, selected
objects, operation semantics, stdout, stderr, file/clipboard output, prompts,
and process exit status. A program returning similar WMI data is insufficient.
Intermediate builds must identify their supported coverage; they must not claim
complete drop-in compatibility merely because common queries work.

The user selected an existing Parallels Windows guest for reference work and
does not require a disposable VM. Tests must use that environment within the
specific operation boundaries described below.

## Platform and compatibility baseline

The project replaces the Windows CLI while continuing to depend on Windows WMI
providers and the caller's permissions. It does not recreate providers missing
from an operating system or promise compatibility with every retired Windows
release.

The initial runtime support proposal is Windows 10 version 1809 and newer and
Windows Server 2019 and newer for x64/x86, and Windows 11 for ARM64. Produce
MSVC builds for `x86_64-pc-windows-msvc`, `i686-pc-windows-msvc`, and
`aarch64-pc-windows-msvc`. Runtime support and provider behavior must be verified
per architecture; successful cross-compilation alone is not runtime evidence.

The first live environment is Windows 11 Pro ARM64, build `26200.8037`, in the
Parallels guest `Windows 11 Home Domain`. Parallels guest execution has been
verified as `NT AUTHORITY\SYSTEM`. Later tests under the user's ordinary
account are a separate dimension. No x64/x86 runtime or remote endpoint has
yet been validated for this project.

Initial inspection found the WMIC executable absent but the `root\cli`
repository present. It contains 81 alias definitions and 10 `MSFT_Cli*` schema
classes. The user indicated that WMIC installation might be necessary, and
installation of the exact advertised `WMIC~~~~` capability is in progress with
automatic restart disabled. Installation success, executable identity, and
resources must be verified before treating it as a behavioral reference.

Pin each reference profile by executable SHA-256, file version, PE architecture,
Windows build, locale, account, and resource inventory. Documentation describes
the candidate surface; the selected reference profile establishes exact
behavior. Differences from other WMIC builds must be recorded explicitly.

## Approach

Use behavior-driven reverse engineering and native Windows management APIs.
Capture the original executable's behavior before implementing each compatible
surface. Consult binary imports, resources, and runtime traces when needed to
resolve ambiguities that documentation and controlled experiments cannot
resolve. Keep observations, hypotheses, and implementation decisions distinct.

Use Microsoft's Rust for Windows bindings to access COM/WMI and other Windows
services. The application must not require PowerShell, .NET, the original WMIC
executable, or restoration of the WMIC optional component at runtime. Normal
Windows system libraries remain dependencies.

## Component boundaries

### 1. Windows entry point and command parser

The Windows entry point retains the original UTF-16 command line and the mode
in which the process was launched. Do not irrevocably remove quotes or replace
unrepresentable UTF-16 before compatibility parsing. A generic Unix CLI parser
must not define the public WMIC grammar.

The parser produces a structured command describing global switches, target,
selector, verb, view, arguments, output modifiers, and help requests. Preserve
lexical information where later interpretation depends on quoting or order.
Represent omitted settings separately from explicit empty/reset values.

Cover case handling, whitespace, quoted names and paths, nested query text,
commas, assignments, object paths, positional alias arguments, method arguments,
and invalid or incomplete input. Determine abbreviations and other accepted
forms experimentally. Accept or reject additional legacy command names such as
`QUERY` and `RESTORE` according to the selected reference; their appearance in
old metadata is not proof of current executable support.

Interactive input and process command lines may have different lexical rules.
Share parsing components where behavior agrees, while retaining explicit entry
contexts for differences. Test real `cmd.exe`, batch, PowerShell, and direct
process invocation independently of parser unit tests.

### 2. Session and execution controller

One session owns effective settings, explicit overrides, output routing,
credentials, and interactive state. The controller resolves an operation,
connects to each selected target, invokes the backend, and supplies typed
results and diagnostics to the renderer.

Model the documented global switches: `/NAMESPACE`, `/ROLE`, `/NODE`,
`/IMPLEVEL`, `/AUTHLEVEL`, `/AUTHORITY`, `/LOCALE`, `/PRIVILEGES`, `/TRACE`,
`/RECORD`, `/INTERACTIVE`, `/FAILFAST`, `/USER`, `/PASSWORD`, `/OUTPUT`,
`/APPEND`, and `/AGGREGATE`. Add any further switches discovered in reference
help and experiments to the compatibility inventory before implementation.

Preserve observed defaults, reset behavior, persistence, command-local effects,
and precedence. This includes node lists and node-list files, repeated queries,
ping checks, aggregation, authentication prompts, and interruption. Execute
multi-node operations in reference order unless measurements establish another
ordering. Do not introduce concurrency that changes output or operation order.

Keep the provider method result, transport/COM error, and process exit status
as separate values. The compatibility policy, based on captured behavior,
determines how each is displayed and how the process exits.

### 3. Alias and schema resolver

An alias is a structured definition with its own connection defaults, target
query, selector template, views, property derivations, verbs, parameters,
qualifiers, and help text. The exported reference data demonstrates positional
`PWhere` substitutions and derived verbs in addition to simple class mappings.

Resolve stock aliases through a versioned built-in definition set so that the
replacement works when the original `root\cli` repository is absent. Also read
role metadata where available to support installed and user-defined aliases.
Missing explicitly selected custom roles must yield the reference error;
silently falling back to the default role would change behavior.

Define precedence among session overrides, alias connection defaults, live
metadata, and packaged defaults from reference experiments. Each precedence
rule must have an observable example in the compatibility corpus before the
associated feature is considered complete.

Support alias views, renamed/derived properties, positional substitutions,
method mappings, and derived command verbs. Interpret `VerbType` using
validated semantics rather than assigning meaning to numeric values by guess.
Derived commands that invoke WMIC must dispatch through this implementation;
they must not depend on finding Microsoft's executable. External utility
derivations must preserve the reference's launch and argument semantics when
invoked by the caller. Discovery must inspect their metadata without executing
them.

Direct `PATH` and `CLASS` access must work for arbitrary provider classes and
instances, including custom namespaces, without adding a hardcoded command per
class. Discover properties, methods, parameter types, and qualifiers through
the WMI schema.

### 4. Native WMI backend

Confine COM/Windows interop to a small adapter with explicit ownership rules.
Initialize and release COM correctly for every participating thread. Initialize
process COM security once, and apply connection/proxy security consistently to
interfaces used for remote calls and enumeration.

The adapter provides namespace connection, class/instance retrieval, WQL
queries, association traversal, schema inspection, method invocation, instance
creation/update/deletion, and class operations accepted by reference commands.
Use the corresponding `IWbemLocator` and `IWbemServices` interfaces. Retain
security, locale, privilege, and provider-context settings needed by the CLI.

Use schema-driven parameter conversion for static and instance methods, named
and positional parameters, omitted/optional values, and output parameters.
Preserve property types, qualifiers, and parameter ordering where observable.

The shared result model retains CIM types and the distinctions between null,
empty string, missing property, empty array, signed/unsigned numbers, booleans,
datetime/interval strings, references, and embedded objects. Do not stringify
all provider values inside the backend.

Represent error code, source, operation, target, and available extended WMI
information separately from localized display text. Match reference rendering
and exit behavior at the compatibility boundary. Preserve behavior for delayed
enumeration failures and partial multi-node results.

Do not add an arbitrary application connection timeout that changes classic
WMIC behavior. A test harness may enforce a timeout, but it must record that as
a harness outcome rather than inventing a WMIC exit status.

### 5. Formatting and output routing

Formatting consumes typed objects and alias/view metadata, and produces the
reference representation. Routing then sends it to the requested destination
with the correct encoding and append/overwrite behavior.

Cover default tables, value lists, CSV, XML/raw XML, MOF, HTML, stock format
names, custom XSL files and parameters, and translation tables. Determine the
canonical XML representation and native XML/XSL engine from the reference
before implementing the custom-format adapter. The full target includes the
custom formatting behaviors accepted by that reference, not only built-in
table and CSV output.

Measure property/instance ordering, name capitalization, alignment, widths,
blank lines, trailing spaces, array syntax, escaping, numeric formatting, and
empty-result behavior. Preserve the ordering effects of `/FORMAT` and
`/TRANSLATE`, and the interaction of `/OUTPUT`, `/APPEND`, and `/RECORD`.

Treat console handles, pipes, redirected files, explicit output files,
recording files, and the clipboard as distinct destinations. Select BOM,
encoding, newline handling, and write APIs based on measured destination
behavior. Never apply a blanket UTF-8 conversion to the compatibility stream.

The normal command stream must contain no extra project banner, telemetry,
debug messages, progress UI, or modernized diagnostics. Keep development logs
outside that stream. Any future extended interface must be explicitly selected
and must not change legacy invocations.

### 6. Interactive shell and contextual help

Launching without a one-shot command must implement the reference session
behavior: startup text, prompt, input/EOF handling, persistent context, reset
rules, contextual `/?`, confirmations, `CONTEXT`, `QUIT`, and `EXIT`.

Help is derived from the alias/schema model where the original does so.
Preserve method/property parameter details, view listings, and help/error
behavior when input is incomplete. Distinguish the interactive shell from the
`/INTERACTIVE` confirmation setting. Test repeated commands and routing changes
within the same process.

## Reverse-engineering and conformance artifacts

Maintain a machine-readable feature inventory and captured cases alongside
human-readable research notes. Each feature records documentation, reference
observations, implementation status, and verification status separately.

Each reference case records:

1. A stable case ID, purpose, operation classification, and prerequisites.
2. Reference profile identity and execution account.
3. Exact UTF-16 command line, launch route, environment, and stdin bytes.
4. Independent raw stdout/stderr bytes, full 32-bit exit status, and whether the
   process completed, timed out, or was cancelled.
5. File/clipboard effects and relevant provider state where applicable.
6. Repeatability notes, any approved normalization, and artifact checksums.

Credentials are an explicit exception to command-line persistence: do not save
real passwords or tokens in reference artifacts or logs. Use controlled test
credentials supplied at execution time for later authentication cases, and
record placeholders in the persisted invocation.

Capture byte streams within Windows before Parallels or shell text processing
can transform them. Base64 is an appropriate transport for those captured
bytes. Console behavior requires console-specific testing and cannot be
inferred from redirected output.

Use stable queries for exact output comparisons. For volatile data, compare
typed results separately and use captured/replayed object data to test
rendering. Normalization must be explicit and restricted to genuinely variable
data; it must not hide spacing, encoding, ordering, or error mismatches.

## Validation on the selected guest

Begin with executable/resource identity, global/contextual help, alias/schema
exports, stable read-only queries, empty results, invalid input, and output
formats. Metadata discovery must not execute every alias or provider method.
In particular, automatic `Win32_Product` enumeration is excluded because
querying it can trigger installer activity; the eventual CLI must still
implement caller-requested access to that alias.

Use the existing selected guest for controlled write tests rather than
requiring a new VM. Create uniquely named test-owned objects, operate only on
their recorded exact identifiers, and clean up those objects after verifying
results. Suitable cases include a test-owned process and a test-owned
environment variable. Do not change unrelated services, accounts, shares,
security settings, registry configuration, or machine power state as part of
automatic conformance discovery.

Generic method dispatch can be validated with schema inspection, parameter
conversion tests, and controlled benign methods. This does not imply that
every destructive provider method has been exercised. Record unexercised
provider-specific behavior honestly. Remote authentication and multi-node
cases require an explicitly authorized remote endpoint and account; this
project currently has only one selected live endpoint.

## Stages and acceptance

### Stage 0: establish the reference

- Verify original WMIC installation, signature, version, hash, architecture,
  available resources, locale, and execution identity.
- Export alias/schema metadata and discover the full help surface.
- Capture initial pipe-output cases as raw bytes with separate stderr and exit
  status, including successful queries, empty results, and errors.
- Establish feature inventory and reference profile provenance.

This stage produces evidence and tooling. It does not claim a replacement
exists.

### Stage 1: native query path

- Implement the Windows entry point, parser, typed backend model, native local
  connection, metadata-aware aliases, direct class/instance targeting, `WHERE`,
  `GET`, `LIST`, initial views, and contextual help needed by those commands.
- Match measured table and value-list output, errors, encodings, and exit codes
  for the stage's case set.
- Keep parser/formatter tests runnable on macOS and verify actual WMI behavior
  using an ARM64 Windows build on the selected guest.

The result is a documented partial compatibility build. Unsupported syntax must
not be accepted and ignored or reported as a successful operation.

### Stage 2: connection and traversal behavior

- Add remote connections, credentials, security switches, custom namespaces and
  roles, node-list files, association traversal, fail-fast checks, repeated
  queries, and aggregation semantics.
- Verify ordering, partial failures, authentication errors, and settings using
  authorized endpoints. Separate implemented from live-verified remote cases.

### Stage 3: methods and writes

- Add schema-driven `CALL`, `CREATE`, `SET`, `DELETE`, required class operations,
  positional alias substitutions, and derived command verbs.
- Verify argument conversion, method output, provider return values,
  confirmations, errors, and cleanup using controlled objects on the selected
  guest.

### Stage 4: full presentation and session behavior

- Complete stock/custom formats, XML/XSL integration, translation tables,
  recording, clipboard and file destinations, and their switch interactions.
- Complete interactive shell/context handling, contextual help, locale behavior,
  interruption, and all discovered reference switches and commands.
- Expand validation to console, pipes, redirection, batch files, and real
  PowerShell invocation.

### Stage 5: compatibility release

- Validate the intended executable architectures and supported Windows profiles.
- Run the accumulated corpus and a set of unchanged real scripts.
- Publish the exact verified scope, known mismatches, provider limitations, and
  installation/path-resolution behavior.
- Do not label the release fully compatible while known command surfaces remain
  unimplemented or required conformance cases are failing.

## Packaging and installation

Build an executable whose Windows filename is `wmic.exe`. Package the stock
alias and built-in formatting definitions needed when the WMIC optional
component and `root\cli` metadata are absent. The original executable and its
installed resource files must not be runtime dependencies.

Use explicit executable paths for development and side-by-side comparisons so
that a test cannot accidentally invoke the replacement as its reference.
Deployment must document PATH resolution and callers that hardcode the old
`System32\wbem\wmic.exe` location. Building a correctly named binary alone does
not resolve those hardcoded paths. Do not overwrite Windows system files as
part of development or test setup.

Keep Cargo dependencies small and pin their resolved versions in `Cargo.lock`.
Use the typed `windows` bindings for this binary application, with the required
API features enabled. Keep unsafe interop localized and document its ownership,
threading, and lifetime invariants. No final package version, public repository,
signing identity, or distribution channel has been selected; those release
decisions do not block local implementation.

## Current artifacts and remaining decisions

The [research inventory](../../research/wmic-compatibility-inventory.md) records
the discovered surface and primary documentation. The
[initial evidence](../../research/evidence/2026-10-15/README.md) preserves the
81 alias definitions and schema exported before installed-WMIC verification.

Measurements still needed before the associated implementations are accepted:

- Installed executable identity and complete help/resource surface.
- Argument parsing, alias/switch precedence, and derived-verb semantics.
- Destination-specific encodings and format/XML/XSL details.
- Reference differences across normal-user versus SYSTEM execution.
- Remote and x64/x86 runtime behavior on authorized environments.

These are research gates with explicit evidence requirements, not assumptions
that the documentation or this specification has already established parity.
The next artifact after written-specification approval is an implementation
plan with ordered tasks, concrete files, checks, and an execution method.

## Primary references

- [WMIC overview and command surface](https://learn.microsoft.com/en-us/windows/win32/wmisdk/wmic)
- [WMIC aliases](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc736307(v=ws.10))
- [WMIC switch semantics](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc787035(v=ws.10))
- [Alias creation and schema](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc785304(v=ws.10))
- [WMIC output formats](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-server-2003/cc757287(v=ws.10))
- [Native WMI namespace connections](https://learn.microsoft.com/en-us/windows/win32/api/wbemcli/nf-wbemcli-iwbemlocator-connectserver)
- [Native WMI method execution](https://learn.microsoft.com/en-us/windows/win32/api/wbemcli/nf-wbemcli-iwbemservices-execmethod)
- [Windows command-line parsing API](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-commandlinetoargvw)
- [Rust for Windows](https://github.com/microsoft/windows-rs)
