# wmic-compat

A native Rust Windows CLI named `wmic.exe`, built to reproduce classic WMIC command behavior. Querying and method execution use WMI COM. Formatting uses the Windows CIM serializer and MSXML6, with built-in styles embedded in the executable.

This is a development compatibility build. The [compatibility report](docs/compatibility.md) separates verified behavior, known differences, and unverified platforms. Full classic compatibility is not yet claimed.

Source and release drafts are hosted at [adamgell/wmic](https://github.com/adamgell/wmic). The [signing workflow](https://github.com/adamgell/wmic/blob/main/docs/signing.md) builds ARM64, x64, and x86, signs through Azure Artifact Signing using OIDC, verifies signatures on Windows, and creates a draft prerelease. A successful run is required before a package can be described as signed.

## Use

Extract the package for your architecture into a directory you own. From PowerShell:

```powershell
.\wmic.exe os get Caption,Version
.\wmic.exe os where "Caption like '%Windows%'" get Caption /value
.\wmic.exe /namespace:\\root\cimv2 path Win32_OperatingSystem get Caption
.\wmic.exe os get Caption,Version /format:csv
.\wmic.exe os get Caption /every:1 /repeat:2
.\wmic.exe CONTEXT
.\wmic.exe /?
.\wmic.exe
```

Starting without arguments enters the classic prompt. `QUIT` or `EXIT` leaves it. `/INTERACTIVE:ON` controls operation confirmations. Query, `CALL`, `CREATE`, `SET`, and `DELETE` commands operate through the current account's WMI permissions.

Use the executable's help for alias properties, methods, formats, and parameters. Aliases retain their metadata-defined connection namespace, which can take precedence over `/NAMESPACE`. An absolute namespace uses the classic leading `\\`; relative namespaces append to the current namespace. Empty values have switch-specific behavior.

## Use existing scripts

Prepend the extracted directory to PATH **for the current process**, then verify resolution:

```powershell
$env:Path = "$PWD;$env:Path"
Get-Command wmic.exe -All
wmic os get Caption
```

A batch file or PowerShell script that calls `wmic` can then use this executable without editing its commands. Current-directory executables and PowerShell aliases/functions can affect resolution, so inspect the result of `Get-Command`.

A literal `C:\Windows\System32\wbem\WMIC.exe` call continues to use that location. PATH cannot redirect it. This project does not overwrite Windows files, install a service, or change system PATH. Keep the Microsoft reference executable available at its own path while comparing behavior.

## Behavior

The parser retains Windows UTF-16 and WMIC's quoting rules. The program preserves separate stdout/stderr and the full 32-bit process exit status. A provider method's `ReturnValue` remains separate from the process status.

Built-in table, value, CSV, MOF, raw XML, XML view, and HTML formats are available, including local XSL parameters, transformation chains, and translation tables. Classic CSV behavior includes unquoted commas and markup escaping. It is not a general RFC CSV writer.

`/OUTPUT`, `/APPEND`, and `/RECORD` use their measured legacy encodings and scope. In the reference profile, redirected stdout uses the console/OEM code page, new output files use UTF-16LE, and records use UTF-8. Routing entered alone persists in the shell; routing placed on a command applies to that command. Other session settings can persist with an inline command.

The bundled metadata contains 81 aliases and 426 views. Live role metadata is used when available. The default role can fall back to the bundled data. Bundled alias introspection is read-only and supports scalar WQL predicates; full missing-namespace profile acceptance remains unverified. Custom roles are not silently replaced by the default role.

External metadata verbs launch their referenced programs only when invoked. Available providers, account rights, operating-system edition, and remote WMI configuration determine what operations can succeed.

## Validation and limits

The reference machine is the selected Windows 11 ARM64 Parallels guest. Read-only captures preserve raw bytes, UTF-16 arguments, executable hashes, independent streams, and full exit statuses. Controlled writes use uniquely owned environment variables and processes, verify identity before mutations, and check cleanup.

Known differences include implementation-specific `/TRACE` text and intentional credential redaction in XML/record command metadata. Volatile OS data remains visible as strict byte differences. Clipboard APIs are implemented, but live clipboard acceptance is pending. Cross-machine authenticated access, other Windows releases, other languages, and class-repository writes are not certified by the selected guest's tests.

An earlier unsigned development build was quarantined by Windows Defender as `Trojan:Win32/Bearfoos.A!ml`. Protection was left enabled and unchanged. Later builds completed the corpus, which does not resolve the earlier detection or establish a false positive. See the compatibility report before broader deployment.

## Build

The repository pins Rust 1.98.1. On Windows, build the appropriate MSVC target:

```text
cargo build --release --target aarch64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc
```

The runtime is linked statically. No Microsoft WMIC executable or installed WMIC stylesheet is a runtime dependency. WMI, COM, MSXML6, and the scripting engine are Windows system components used by the implementation.

On an Apple Silicon development host, the verified cross-build setup uses cargo-xwin 0.23.1 and LLVM. Install the desired Rust targets, then:

```sh
XWIN_ARCH=x86,x86_64,aarch64 PATH="/opt/homebrew/opt/llvm/bin:$PATH" \
  cargo xwin build --release --target aarch64-pc-windows-msvc
```

Use the other target triples for x64 and x86. Compilation is separate from runtime acceptance.

## Test and package

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 -m unittest discover -s tools/reference/tests
python3.12 -m unittest discover -s tools/release/tests
pwsh -File tools/release/tests/signature-policy.Tests.ps1
```

The reviewed application baseline passes 158 portable Rust tests and 21 native Windows tests. The [compatibility report](docs/compatibility.md) pins the measured baseline executable hashes, corpus results, and remaining acceptance limits. Release automation requires Python 3.11 or later; CI uses Python 3.12.

For the selected Parallels guest, run the read-only corpus against a release build:

```sh
python3 tools/reference/run_corpus.py \
  --vm '{ec3bc428-7a31-4f17-8424-7ce78923f7f3}' \
  --target aarch64-pc-windows-msvc \
  --binary target/aarch64-pc-windows-msvc/release/wmic.exe \
  --out artifacts/recheck-arm64
```

The report retains known differences; it does not turn them into passing comparisons. Use a fresh output directory for each measured build. x86 runs capture the installed x86 original first; x64 runs use the ARM64 original and remain cross-architecture comparisons.

Windows-only integration tests are ignored by default and require the reference fixtures and a selected guest. The source under `tools/windows` contains the controlled harnesses; read their scope before running them. The CLI itself does not require Python or PowerShell for ordinary WMI operations.

```sh
python3 tools/release/package.py \
  --target aarch64-pc-windows-msvc \
  --binary target/aarch64-pc-windows-msvc/release/wmic.exe \
  --out artifacts/packages
python3 tools/release/package.py --verify artifacts/packages/wmic-compat-0.1.0-dev-aarch64-pc-windows-msvc.zip
```

Packages contain the executable, usage and compatibility documents, and SHA-256 manifests. Reference binaries, credentials, test captures, and research copies of Microsoft's stylesheets are excluded. Packaging is local and does not publish or install anything.
