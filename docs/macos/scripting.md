# Scripting with macOS wmic

The macOS executable shares WMIC's command language with the Windows build. Its process boundary follows Unix conventions: argv is reconstructed from shell arguments, standard streams are UTF-8, and the process exit code has eight bits. This guide covers the places that matter when moving a command into a script.

The [cookbook](cookbook.md) supplies query examples. The [examples directory](../../examples/macos/README.md) contains complete scripts that can run from a checkout.

## Select the executable explicitly

From the repository root:

```sh
export WMIC_BIN="$PWD/target/release/wmic"
"$WMIC_BIN" os get Caption,Version /value
```

Use a quoted executable variable, not `eval`. A path containing spaces then remains one argument. The supplied scripts accept `WMIC_BIN`; the Python scripts also accept `--wmic`.

## Shell quoting

There are two parsers: zsh/Bash processes your command first, then WMIC parses the arguments it receives. Quote the complete WHERE expression so it reaches WMIC as one argument:

```sh
"$WMIC_BIN" process where "Name='Finder' AND ProcessId > 1" get ProcessId,Name
"$WMIC_BIN" logicaldisk where "DeviceID='/'" get Size,FreeSpace /value
```

Single quotes around a shell argument preserve its contents literally. Double quotes allow shell variables to expand while preserving the argument boundary:

```sh
"$WMIC_BIN" path '\\.\root\cimv2:Win32_OperatingSystem=@' get Caption
"$WMIC_BIN" process where "ProcessId=$$" get Name,ProcessId
"$WMIC_BIN" path "Win32_Process.Handle=\"$$\"" get Name
```

Quote wildcard and help syntax in zsh:

```sh
"$WMIC_BIN" os get '*' /value
"$WMIC_BIN" '/?'
```

Put a switch with a spaced value in one shell argument:

```sh
report_dir=$(mktemp -d "${TMPDIR:-/tmp}/wmic-guide.XXXXXX")
"$WMIC_BIN" "/output:$report_dir/OS report.txt" os get Caption,Version /value
```

WMIC preserves the `/output:` part while quoting its value during argv reconstruction. This is different from composing an unquoted string and asking the shell to split it again.

For comma-separated switch values, preserve any quotes that belong to WMIC itself. Quote each node separately, for example `'/node:"a","b"'`. The original node scanner also splits commas inside quotes, so quoting the whole list (`'/node:"a,b"'`) can consume the following command as part of the node value. A hyphenated node needs retained quotes such as `'/node:"host-name"'`. These forms do not enable remote querying on macOS. The [quoting reference](../macos.md#shell-quoting) lists the remaining reconstruction limits.

## Process status and method results

Check the command's exit status directly:

```sh
if "$WMIC_BIN" os get Caption /value > "$report_dir/os.txt" 2> "$report_dir/os.stderr.txt"; then
    printf '%s\n' 'Query completed'
else
    wmic_rc=$?
    printf 'wmic failed with POSIX status %s\n' "$wmic_rc" >&2
    exit "$wmic_rc"
fi
```

Do not capture `$?` after another command. Do not use `if ! command; then rc=$?`; that captures the status of the negation. A pipeline also normally exposes the last command's status, so a successful parser can hide a failed WMIC command. Capture and check WMIC before parsing, or use your shell's pipeline-status facilities deliberately.

| Result | Meaning |
| --- | --- |
| CLI exits 0 | The command completed according to the CLI. It can still have selected no instances. |
| CLI exits nonzero | The 32-bit WMIC status mapped to a POSIX exit code. |
| Method prints `ReturnValue = 0;` | The provider method reports success or acceptance. |
| Method prints another `ReturnValue` | A method-specific result; CLI exit 0 alone does not establish method success. |

For example, `GetOwner` returns its account data as out parameters. A process that disappears can produce an empty selection or a method failure. The [owned-process Python example](../../examples/macos/process-lifecycle.py) checks both process status and each method result.

The low byte of a nonzero WMIC status becomes the process exit code; a zero low byte becomes 1. Examples include `Invalid class` 0x80041010 → 16, `Invalid query` 0x80041017 → 23, and `Invalid namespace` 0x8004100E → 14. The full status cannot be recovered from `$?` alone. A cancelled CLI ends by SIGINT, commonly reported by the shell as 130.

Stderr can contain an empty-result diagnostic or formatting newline even when the command succeeds. Test the exit status and expected result shape rather than treating any stderr byte as failure.

## File encodings

Shell redirection captures the normal Unix stream:

```sh
"$WMIC_BIN" os get Caption,Version /value > "$report_dir/os-utf8.txt"
```

That file is **UTF-8 with LF**. By contrast, WMIC's explicit destination switch retains the legacy file format:

```sh
"$WMIC_BIN" "/output:$report_dir/os-utf16.txt" os get Caption,Version /value
"$WMIC_BIN" "/append:$report_dir/os-utf16.txt" os get Caption,Version /value > /dev/null
iconv -f UTF-16 -t UTF-8 "$report_dir/os-utf16.txt"
```

| Destination | Encoding |
| --- | --- |
| stdout/stderr, including shell `>` | UTF-8, LF |
| `/OUTPUT` and a new `/APPEND` file | UTF-16LE BOM, CRLF |
| `/APPEND` to an existing UTF-16LE BOM file | UTF-16LE, without adding another BOM |
| `/APPEND` to an existing file without a BOM | UTF-8, CRLF |
| `/APPEND` to an existing UTF-8 BOM file | Appended text is UTF-16LE; the existing prefix/content is retained |
| `/RECORD` | UTF-8 |

`/APPEND` can also leave normal query output on stdout, which is why the append example redirects that stream when only the file is wanted. Do not pre-create an empty append destination if you intend it to be a new UTF-16LE file: an existing empty file has no BOM and selects the UTF-8 append path.

The UTF-8 BOM case is a legacy destination-selection quirk: append does not transcode the existing file, so it can mix old UTF-8 bytes with new UTF-16LE text. Use a new destination or a known UTF-16LE BOM file when you want a consistently encoded legacy output file.

The [inventory script](../../examples/macos/inventory.sh) uses shell redirection so its reports are directly readable by Unix tools. It creates a unique report directory and does not reuse existing report filenames.

The temporary `report_dir` above is yours to retain or remove after inspection. These examples never write a report into a system configuration directory.

## Parsing values and producing JSON

Choose a small projection with properties you understand. A singleton `/value` query is convenient for OS version fields:

```sh
"$WMIC_BIN" os get Caption,Version,BuildNumber,OSArchitecture /value
python3 examples/macos/os-summary.py --wmic "$WMIC_BIN"
```

The Python example uses the standard library, checks the CLI status, splits each selected `Name=Value` line at its first equals sign, requires all four expected fields, and writes a JSON object of strings. It is intentionally specific to those scalar OS fields. It does not claim to parse arbitrary multiline records, arrays, or CIM types, and it does not add a native `/format:json` mode.

For multi-instance data, preserve record boundaries and types deliberately. The classic CSV format can contain unquoted commas and legacy escaping; `cut -d,`, a naïve `split(',')`, or an ordinary CSV reader can misinterpret it. Raw XML retains a richer shape; custom XSLT can project a controlled output. See the [stylesheet example](../../examples/macos/os-summary.xsl).

## Interactive and redirected sessions

No arguments starts the WMIC prompt. A file on stdin also goes through that session parser:

```sh
"$WMIC_BIN" < examples/macos/session.txt
```

The supplied file contains read-only commands and `QUIT`. Prompt text can appear on stdout, so redirected interactive output is not a clean data stream. Use individual commands for machine parsing.

A here-document avoids shell expansion when its delimiter is quoted:

```sh
"$WMIC_BIN" <<'WMIC_COMMANDS'
CONTEXT
os get Caption,Version /value
cpu get Name
QUIT
WMIC_COMMANDS
```

These lines are WMIC commands, not shell commands. Do not add shell `#` comments, shell redirections, or `$variable` references expecting WMIC to interpret them.

Routing entered as a standalone command persists within that session. Routing attached to a query applies to that query. For example, with `report_dir` set to the temporary directory created earlier:

```sh
"$WMIC_BIN" <<WMIC_COMMANDS
/output:"$report_dir/session-output.txt"
os get Caption /value
os get Version /value
/output:stdout
QUIT
WMIC_COMMANDS
```

This delimiter is intentionally unquoted so the shell expands `$report_dir` before WMIC reads it. The destination file uses the `/OUTPUT` UTF-16LE format. If a command fails, the session can continue; a later success replaces its last status, and `QUIT` resets it. Do not use a final session exit 0 as proof that every earlier line succeeded.

## Automation and sampling

Use an absolute executable path and explicit output location in a scheduler or management agent. Its PATH, inherited environment, user identity, access to the GUI session, and home directory can differ from an interactive Terminal.

Keep sampling finite:

```sh
"$WMIC_BIN" cpu get LoadPercentage /every:1 /repeat:3
```

An unbounded `/EVERY` invocation needs cancellation. Clipboard destinations need a graphical session and are a poor choice for unattended inventory. Service inventory can invoke several `launchctl` commands with individual deadlines; allow for that in an outer timeout.

Run inventory as the ordinary user unless you specifically need privileged process fields. Running under `sudo` changes the environment and account context. It does not enable remote nodes or implement unsupported verbs.

Power operations and experimental service mutations are not part of the inventory scripts. See the [method reference](../macos.md#methods) before choosing to invoke either; no real power operation has acceptance coverage.

## Examples and verification

From the repository root:

```sh
export WMIC_BIN="$PWD/target/release/wmic"
sh examples/macos/inventory.sh ./artifacts
python3 examples/macos/os-summary.py
python3 examples/macos/process-lifecycle.py
"$WMIC_BIN" < examples/macos/session.txt
"$WMIC_BIN" os get Caption,Version '/format:"examples/macos/os-summary.xsl"'
```

The inventory and summary are read-only. The lifecycle script creates and terminates only its own temporary child and requires no root access. The session and stylesheet contain read-only queries. The examples use no third-party Python modules.

Application verification commands and the limits of CI versus live acceptance remain in the [macOS reference](../macos.md#verification) and [testing guide](../testing.md).
