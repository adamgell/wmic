## A Rust replacement for classic WMIC

For anyone with scripts that still call `wmic`, this provides a standalone `wmic.exe` implemented in Rust. It aims to reproduce classic WMIC's commands, output, quoting, exit codes, and interactive prompt so existing batch files, PowerShell scripts, and command-line tools can keep using familiar invocations.

Queries and methods run through native Windows WMI/COM. The executable does not require Microsoft's original WMIC binary, Python, or a PowerShell wrapper to perform ordinary WMI operations.

### What it does

- **Query Windows system information:** operating system, hardware, processes, services, disks, and other available WMI providers, using familiar aliases such as `os`, `cpu`, `process`, and `service`.
- **Work with WMI classes directly:** `PATH`, `CLASS`, WQL filters, schema help, and associations. The bundled metadata includes 81 aliases and 426 views.
- **Invoke methods and change WMI objects:** `CALL`, `CREATE`, `SET`, and `DELETE`, subject to the current account's permissions and the provider's capabilities.
- **Produce classic output formats:** table, value, CSV, MOF, XML, and HTML, with built-in formatting and support for custom XSL and translations.
- **Support command-line workflows:** interactive mode, connection options, repeated queries, output files, append, recording, and separate stdout/stderr with full Windows process exit statuses.

### Try it

Extract the ZIP for your Windows architecture, then run from PowerShell:

```powershell
.\wmic.exe os get Caption,Version
.\wmic.exe cpu get Name
.\wmic.exe process get Name,ProcessId
.\wmic.exe os get Caption,Version /format:csv
.\wmic.exe /?
```

Run `wmic.exe` without arguments to enter the interactive prompt. To use existing scripts, put the extracted directory first in the calling process's PATH and verify which `wmic.exe` resolves. Calls with a hardcoded Windows system path still use that path. See the [README](https://github.com/adamgell/wmic#readme) for setup and examples.

### Downloads and signing

Signed builds are available for **Windows ARM64, x64, and x86**. Each executable is signed as **Adam Gell** through Azure Artifact Signing and carries a Microsoft timestamp. Archive checksums, signature verification receipts, and source/run provenance are attached to this release.

### Development status

This is **v0.1.0-dev**. Full classic WMIC compatibility is not claimed. The [compatibility report](https://github.com/adamgell/wmic/blob/main/docs/compatibility.md) describes the measured baseline, known differences, and unverified Windows versions, languages, hardware, remote-authentication scenarios, and provider operations. The signed release builds have separate hashes and do not expand that runtime evidence.

An earlier unsigned development build was quarantined by Defender as `Trojan:Win32/Bearfoos.A!ml`. That historical detection remains unresolved. Code signing and later successful tests do not establish a false positive or clear the earlier detection.
