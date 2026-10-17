# Getting started on macOS

`wmic` gives macOS a familiar WMIC command interface. It reads local macOS APIs and presents Win32-shaped records. You can query a Mac using commands such as `wmic cpu get Name`; this does not install WMI or connect to a Windows machine.

The macOS build is currently distributed as source. The signed archives on [GitHub Releases](https://github.com/adamgell/wmic/releases) contain Windows executables. They cannot run as native macOS programs.

## Prerequisites

The tested platform is Apple Silicon. CI covers macOS 15 and macOS 26 on arm64. Intel Macs and older macOS releases remain unverified.

You need:

- Rust **1.98.1 or later**, matching `rust-version` in [Cargo.toml](../../Cargo.toml).
- Apple's Xcode Command Line Tools or an appropriate full Xcode installation.
- Git to clone the source.

Install Rust using the [official rustup instructions](https://rust-lang.org/tools/install/). If you already manage Rust with rustup, update your stable toolchain when needed:

```sh
rustup update stable
rustc --version
cargo --version
```

Apple documents the Command Line Tools installation command in its [command-line tools FAQ](https://developer.apple.com/library/archive/technotes/tn2339/_index.html):

```sh
xcode-select --install
```

Complete the installation dialog before building. If the tools are already installed, check the selected developer directory:

```sh
xcode-select --print-path
xcrun --find clang
```

The build links Apple's system libraries and frameworks, including libxml2 and libxslt. You do not need Homebrew, pkg-config, Python, PowerShell, a Windows VM, or a Microsoft WMIC installation to build and use ordinary macOS queries.

## Build and run from a checkout

```sh
git clone https://github.com/adamgell/wmic.git
cd wmic
cargo build --locked --release
./target/release/wmic os get Caption,Version /value
```

The executable is `target/release/wmic`. `--locked` requires Cargo to use the committed dependency lock file. The repository has no `rust-toolchain.toml`; your selected compiler must satisfy `Cargo.toml`.

The first query should print `Caption=` and `Version=` records. For example, the shape is:

```text
Caption=macOS <version>
Version=<version>
```

The values, spacing, and property order come from your system and the selected format. Example output in these guides explains the shape; it is not a fixture for your Mac.

## Optional installation for your user

You can keep running the executable from the checkout. To place a copy in a directory owned by your account:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/wmic "$HOME/.local/bin/wmic"
export PATH="$HOME/.local/bin:$PATH"
command -v wmic
wmic os get Caption,Version /value
```

`install` replaces an existing `wmic` at that destination. The `export` applies to the current shell. To keep this PATH entry, add that export line once to the startup file you use, such as `~/.zshrc`, then open a new shell. No system directories or launchd jobs are involved.

After updating the checkout, rebuild and repeat the `install` command to update that copy. `cargo build` alone updates the checkout's executable, not a previously installed copy.

## Your first five queries

Identify the OS:

```sh
wmic os get Caption,Version,BuildNumber,OSArchitecture /value
```

Inspect the CPU and aggregate memory:

```sh
wmic cpu get Name,NumberOfCores,NumberOfLogicalProcessors
wmic computersystem get Model,SystemType,TotalPhysicalMemory /value
```

Inspect the root filesystem and network interfaces:

```sh
wmic logicaldisk where "DeviceID='/'" get DeviceID,FileSystem,Size,FreeSpace
wmic nic get Index,NetConnectionID,MACAddress,NetEnabled
```

`TotalPhysicalMemory`, filesystem `Size`, and `FreeSpace` use bytes. OS properties such as `FreePhysicalMemory` use KiB; the [cookbook](cookbook.md#memory) explains the distinction.

## Discover commands and properties

```sh
wmic '/?'
wmic os '/?'
wmic os get '/?'
wmic process call '/?'
wmic alias list brief
wmic class Win32_Process
```

Quote `/?` if your shell treats it as a filename pattern:

```sh
wmic '/?'
```

Help and schema descriptions are drawn from the Windows metadata. They can list aliases, methods, properties, and write qualifiers that the macOS backend does not implement. Use the [supported class table](../macos.md#supported-classes) and [method table](../macos.md#methods) to check actual support. For example, an unsupported alias can have help while its query returns `Invalid class`.

## Interactive mode

Start the program with no arguments:

```sh
wmic
```

At the `wmic:` prompt, enter WMIC commands without a leading `wmic`:

```text
CONTEXT
os get Caption,Version /value
cpu get Name
QUIT
```

`EXIT` also leaves the prompt. `CONTEXT` shows session settings. Commands entered here are parsed by WMIC, not zsh or Bash: shell variables such as `$HOME` are not expanded. See [sessions and automation](scripting.md#interactive-and-redirected-sessions).

## What carries over from Windows

The parser, alias metadata, help, formats, and session machinery are shared with the Windows build. Local queries cover 30 Win32 classes. The data has macOS meaning: disks have mount points rather than drive letters, services have launchd labels, and `Product` enumerates application bundles rather than Windows Installer products.

The macOS backend supports local `root\cimv2` queries. Remote Windows WMI connections, `ASSOC`, and the generic `SET`, `CREATE`, and `DELETE` verbs are unsupported. `process call Create` is a separate implemented method; it is not the generic `CREATE` verb.

Most queries work as your ordinary user. Some fields for other users' processes are NULL. Service control is experimental. Power methods can shut down the machine and have only injected test coverage, so they are excluded from the runnable examples.

## Troubleshooting

| Symptom | Check or next action |
| --- | --- |
| `wmic: command not found` | Use `./target/release/wmic`, or check `command -v wmic` and PATH. |
| Cargo reports an unsupported Rust version | Compare `rustc --version` with `rust-version` in `Cargo.toml`; select a sufficiently new compiler. |
| `xcrun` reports an invalid developer path | Check `xcode-select --print-path` and complete the Apple tools installation. |
| The program is a Windows `.exe` | Build the source for macOS; release archives currently target Windows. |
| zsh says `no matches found` | Quote arguments containing wildcard syntax, for example `wmic os get '*'` or `wmic '/?'`. |
| `Invalid class` or `Not found` | Check the class/provider table. Windows help does not imply a macOS provider exists. |
| `Invalid query` | Verify property names and quote the whole WHERE expression. |
| `Invalid namespace` | Use the local `root\cimv2` namespace; Windows namespaces such as `root\cli` are not system providers here. |
| `The RPC server is unavailable` | A remote node was selected. This build queries the local Mac only. |
| A value is blank | The property may be NULL or unavailable to your user. An empty class can also be a valid result. |
| A text editor shows NUL characters | `/OUTPUT` files use UTF-16LE. Shell `>` redirection produces UTF-8; see [file encodings](scripting.md#file-encodings). |
| Method invocation exits 0 but reports a nonzero `ReturnValue` | The CLI invocation succeeded, but the provider method did not. Inspect both results. |

For a useful issue report, include the source commit (`git rev-parse HEAD`), macOS version and architecture (`sw_vers` and `uname -m`), the exact command with secrets removed, stdout/stderr, and the shell exit code. Report whether the command ran as your user or under `sudo`.

Continue with the [command cookbook](cookbook.md), [scripting guide](scripting.md), or [runnable examples](../../examples/macos/README.md).
