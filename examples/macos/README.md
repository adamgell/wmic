# Runnable macOS examples

These examples use the source-built `wmic` and the macOS backend. Run them from the repository root after building:

```sh
cargo build --locked --release
export WMIC_BIN="$PWD/target/release/wmic"
```

`WMIC_BIN` can be an absolute path containing spaces. If omitted, the scripts resolve `wmic` on PATH. The Python examples use only the Python 3 standard library; the application itself does not require Python.

| Example | What it demonstrates | Effects |
| --- | --- | --- |
| [inventory.sh](inventory.sh) | Seven focused queries, Unix redirection, unique output directory | Creates report files; inventory queries are read-only |
| [os-summary.py](os-summary.py) | A checked singleton query projected into JSON | Read-only |
| [process-lifecycle.py](process-lifecycle.py) | Process selection, `GetOwner`, `Terminate`, method result checks | Starts and terminates only its own temporary child |
| [session.txt](session.txt) | Commands through the interactive parser | Read-only |
| [os-summary.xsl](os-summary.xsl) | A local XSLT 1.0 text projection | Read-only |

No example requests a shutdown/restart, changes launchd jobs, uses `sudo`, or changes system settings. Service control remains experimental; see the [macOS reference](../../docs/macos.md#methods).

## Inventory collection

```sh
sh examples/macos/inventory.sh ./artifacts
```

The script creates a unique `wmic-inventory.XXXXXX` directory below the supplied parent. Its seven files cover OS, CPU, computer identity/model, logical disks, adapters, network configuration, and process names/IDs. All reports are UTF-8 with LF line endings because they capture stdout using shell redirection.

The directory is created with private permissions through `umask 077` and `mktemp`. Existing report files are not reused. A failed command stops the script and leaves the partial directory for inspection; the directory location is printed to stderr before querying. Complete runs print the directory path to stdout, so a caller can capture it:

```sh
inventory_dir=$(sh examples/macos/inventory.sh ./artifacts)
printf 'Reports are in %s\n' "$inventory_dir"
```

Treat these files as inventory: they include the machine name, IP/MAC addresses, and process names. They deliberately omit complete process command lines, complete environments, and hardware serial-number queries.

## JSON OS summary

```sh
python3 examples/macos/os-summary.py
```

Or select an executable without setting an environment variable:

```sh
python3 examples/macos/os-summary.py --wmic ./target/release/wmic
```

The JSON contains four string-valued keys: `Caption`, `Version`, `BuildNumber`, and `OSArchitecture`. The script checks the CLI status and requires every selected property exactly once and nonempty. It is a small wrapper around a singleton query, not a general CIM serializer or native JSON format.

## Owned-process lifecycle

```sh
python3 examples/macos/process-lifecycle.py
```

The script starts `/bin/sleep 120`, captures that child's PID, and queries only `ProcessId=<that PID>`. It calls `GetOwner`, then `Terminate`, requires exactly one successful `ReturnValue` from each method, and checks that the child ended by SIGKILL. A `finally` block cleans up the child if an earlier step fails. CLI subprocess calls have timeouts.

The child is created with Python's process API so the example retains its parent/child handle and can reap it. The cookbook also shows the distinct [WMIC `Create` method](../../docs/macos/cookbook.md#process-creation-and-termination) using a short-lived `/usr/bin/true` process.

`Terminate` uses SIGKILL. This script never selects processes by a common name or accepts an arbitrary target PID. Run it as your ordinary user.

## Redirected session

```sh
"$WMIC_BIN" < examples/macos/session.txt
```

The file contains WMIC commands, not shell syntax. Output includes interactive prompt text. `QUIT` ends the session and resets its final status, so use direct invocations when you need to check each command independently. See [session behavior](../../docs/macos/scripting.md#interactive-and-redirected-sessions).

## Custom stylesheet

```sh
"$WMIC_BIN" os get Caption,Version '/format:"examples/macos/os-summary.xsl"'
```

The stylesheet projects the two selected scalar properties into `Caption=...` and `Version=...` lines. It uses the system libxslt engine without scripts, external documents, or network requests. System-provided libxslt output can differ from MSXML6 for some serializations; see the [custom stylesheet limits](../../docs/macos.md#formats-and-stylesheets).

## More examples

- [Getting started](../../docs/macos/getting-started.md)
- [Command cookbook](../../docs/macos/cookbook.md)
- [Scripting guide](../../docs/macos/scripting.md)
- [Class, method, and acceptance reference](../../docs/macos.md)
