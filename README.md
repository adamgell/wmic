# wmic-compat

A native Rust command-line tool, built to reproduce classic WMIC command behavior. On Windows the executable is `wmic.exe`; querying and method execution use WMI COM, and formatting uses the Windows CIM serializer and MSXML6, with built-in styles embedded in the executable. On macOS the executable is `wmic`, built from source, and queries are answered from macOS system interfaces. See [macOS](#macos).

This is a development compatibility build. The [compatibility report](docs/compatibility.md) separates verified behavior, known differences, and unverified platforms. Full classic compatibility is not yet claimed.

Download signed Windows ARM64, x64, and x86 packages from [GitHub Releases](https://github.com/adamgell/wmic/releases). Each package includes `wmic.exe`, usage and compatibility documents, and SHA-256 manifests. See the [signing documentation](https://github.com/adamgell/wmic/blob/main/docs/signing.md) for signature verification and release provenance.

## Use

On Windows, extract the package for your architecture into a directory you own. From PowerShell:

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

## macOS

The macOS build is `wmic`. It uses the same command parser, help, aliases, and output formats as the Windows build. It answers queries from macOS system interfaces instead of WMI. It serves 30 Win32 classes in `root\cimv2` for the local machine. `SET`, `CREATE`, `DELETE`, and `ASSOC` are not supported. A remote node is queried over WinRM (see below). No Microsoft WMIC exists for macOS, so data values have no reference corpus. The [macOS guide](https://github.com/adamgell/wmic/blob/main/docs/macos.md) lists the classes, methods, remote nodes, evidence, and unverified areas. The examples assume `wmic` is on PATH.

```sh
wmic os get Caption,Version
wmic os where "Caption like '%macOS%'" get Caption /format:csv
wmic process where "ProcessId=$$" call GetOwner
```

A `/node` that is not the local machine sends the request to the WinRM service on that node (WS-Management over HTTP, port 5985, or over HTTPS, port 5986, with `WMIC_WSMAN_SCHEME=https`). A local node with `/user` is refused, as on Windows, unless `WMIC_WSMAN_PORT` names a listener. The macOS build cannot speak DCOM, so the target needs WinRM enabled (`Enable-PSRemoting` or `winrm quickconfig`). Authentication is NTLMv2 with message encryption. Over HTTPS, the server certificate is validated against the platform trust store, plus a PEM file named by `WMIC_WSMAN_CA_FILE`, and the logon is bound to the certificate. Basic, Kerberos, and CredSSP are not implemented. Give the account as `/user:DOMAIN\name`, or as `/user:name /authority:NTLMDOMAIN:domain`. Without `/password:`, `wmic` reads the password from standard input.

```sh
wmic /node:server01 '/user:CONTOSO\admin' os get Caption
```

Quote the account argument so the shell keeps the backslash.

The environment variable `WMIC_WSMAN_PORT` replaces the default port. The [remote nodes section](https://github.com/adamgell/wmic/blob/main/docs/macos.md#remote-nodes) of the macOS guide lists the requirements, errors, and limits. The Windows build is unchanged and uses DCOM.

The shell removes quotes before `wmic` starts. `wmic` puts quotes around an argument that has white space. To pass quotes to WMIC, put the whole argument in single quotes: `wmic '/node:"a,b"' os get Caption`.

A POSIX exit code has 8 bits and a WMIC status has 32 bits. Status 0 stays 0. A cancelled command ends by SIGINT. Any other status exits with its low byte, and a low byte of 0 becomes 1. For example, `0x80041010` exits with 16 and `0x80070005` exits with 5.

Standard output and standard error are UTF-8 with LF line endings. Without root, some values of other users' processes are empty. Service start and stop in the system domain, and reboot and shutdown, need root.

macOS service control (`StartService`, `StopService`, and `ChangeStartMode`) is experimental. Its command construction has unit tests, but a real test-owned LaunchAgent lifecycle has not been verified. Treat it as unverified until start, stop, KeepAlive, enable/disable, subsequent inventory and cleanup all have live acceptance coverage.

Start with the [macOS getting-started guide](docs/macos/getting-started.md), then use the [command cookbook](docs/macos/cookbook.md) and [scripting guide](docs/macos/scripting.md). The [runnable examples](examples/macos/README.md) include inventory collection, a JSON summary, a process lifecycle limited to its own child, and a redirected interactive session.

## Behavior

This section describes the Windows build. The [macOS guide](https://github.com/adamgell/wmic/blob/main/docs/macos.md) lists the macOS differences.

The parser retains Windows UTF-16 and WMIC's quoting rules. The program preserves separate stdout/stderr and the full 32-bit process exit status. A provider method's `ReturnValue` remains separate from the process status.

Built-in table, value, CSV, MOF, raw XML, XML view, and HTML formats are available, including local XSL parameters, transformation chains, and translation tables. Classic CSV behavior includes unquoted commas and markup escaping. It is not a general RFC CSV writer.

`/OUTPUT`, `/APPEND`, and `/RECORD` use their measured legacy encodings and scope. In the reference profile, native pipes use the console/OEM code page; formatted queries redirected to a disk file and new `/OUTPUT` files use UTF-16LE. Records use UTF-8. Help and diagnostic output keep their measured byte paths. Routing entered alone persists in the shell; routing placed on a command applies to that command. Other session settings can persist with an inline command.

The bundled metadata contains 81 aliases and 426 views. Live role metadata is used when available. The default role can fall back to the bundled data. Bundled alias introspection is read-only and supports scalar WQL predicates; full missing-namespace profile acceptance remains unverified. Custom roles are not silently replaced by the default role.

External metadata verbs launch their referenced programs only when invoked. Available providers, account rights, operating-system edition, and remote WMI configuration determine what operations can succeed.

## Validation and limits

On Windows, compatibility is checked against classic Microsoft WMIC using captures of raw output bytes, UTF-16 arguments, executable hashes, independent streams, and full exit statuses. Controlled write tests verify the identity and cleanup of their own test objects. The [compatibility report](docs/compatibility.md) describes the tested Windows profile and the evidence behind each result.

Known differences include implementation-specific `/TRACE` text and intentional credential redaction in XML/record command metadata. Volatile OS data remains visible as strict byte differences. Clipboard APIs are implemented, but live clipboard acceptance is pending. Cross-machine authenticated access, additional Windows releases and languages, and class-repository writes remain unverified.

An earlier unsigned development build was quarantined by Windows Defender as `Trojan:Win32/Bearfoos.A!ml`. Protection was left enabled and unchanged. Later builds completed the corpus, which does not resolve the earlier detection or establish a false positive. See the compatibility report before broader deployment.

## Build

Use Rust 1.98.1 or later, as declared by `rust-version` in `Cargo.toml`. On Windows, build the appropriate MSVC target:

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

On macOS, the Xcode Command Line Tools are enough. Homebrew and pkg-config are not needed. The build links the system libxml2 and libxslt.

```sh
cargo build --release
```

The executable is `target/release/wmic`. The build is verified on an Apple Silicon Mac mini that runs macOS 26.6.2. CI builds on macOS 15 and macOS 26 (both arm64). Intel Macs and older macOS versions are unverified.

## Test and package

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 -m unittest discover -s tools/reference/tests
python3.12 -m unittest discover -s tools/release/tests
pwsh -File tools/release/tests/signature-policy.Tests.ps1
```

The [testing guide](docs/testing.md) describes the reproducible 2,085-case oracle corpus, strict comparison gates, and 121 controlled cases per architecture. The [compatibility report](docs/compatibility.md) pins the measured baseline executable hashes, corpus results, and remaining acceptance limits, including the separately measured 286-case switch-value suite and [237 REA regression cases](docs/research/rea-25-passes.md). Release automation requires Python 3.11 or later; CI uses Python 3.12.

On macOS, `cargo test --locked` also runs the macOS suites. The [macOS guide](docs/macos.md) lists their scope, evidence, and remaining acceptance limits.

Reference capture and Windows integration tests require the appropriate fixtures and an explicitly selected test environment. See the [reference tooling](https://github.com/adamgell/wmic/tree/main/tools/reference) and [controlled Windows harnesses](https://github.com/adamgell/wmic/tree/main/tools/windows) for their setup and scope. The CLI itself does not require Python or PowerShell for ordinary WMI operations.

```sh
python3 tools/release/package.py \
  --target aarch64-pc-windows-msvc \
  --binary target/aarch64-pc-windows-msvc/release/wmic.exe \
  --out artifacts/packages
python3 tools/release/package.py --verify artifacts/packages/wmic-compat-0.1.0-dev-aarch64-pc-windows-msvc.zip
```

The packaging script builds Windows packages only. Windows packages contain the executable, usage and compatibility documents, and SHA-256 manifests. Reference binaries, credentials, test captures, and research copies of Microsoft's stylesheets are excluded. Packaging is local and does not publish or install anything.
