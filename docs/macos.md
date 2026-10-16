# macOS build

This document describes the macOS build of `wmic`. It states what the build does, how to use it, and what evidence supports each claim. The evidence differs from the Windows build. See [Verification](#verification) and [Not verified](#not-verified).

## Documentation map

| Start here | What it covers |
| --- | --- |
| [Getting started](macos/getting-started.md) | Build, optional installation, first queries, help, and troubleshooting |
| [Command cookbook](macos/cookbook.md) | Hardware, storage, networking, processes, apps, accounts, filters, paths, and formats |
| [Scripting guide](macos/scripting.md) | zsh/Bash quoting, exit codes, method results, encodings, JSON, sessions, and automation |
| [Runnable examples](../examples/macos/README.md) | A read-only inventory script, JSON summary, an owned-process lifecycle, and a session file |
| This reference | The complete class/provider table, value mappings, methods, and acceptance limits |

For a first query from a source checkout:

```sh
cargo build --locked --release
./target/release/wmic os get Caption,Version /value
```

The commands in the guides use `wmic` after you put the executable on PATH. Replace it with `./target/release/wmic` to use the checkout directly. Ordinary queries need neither root nor Python.

## Scope

The macOS build runs the same parser, help, alias set, formats, and session logic as the Windows build. It does not use WMI. For the local machine, a backend answers each query from macOS system interfaces: sysctl, Mach, libproc, IOKit, Core Foundation, SystemConfiguration, `getfsstat`, `getifaddrs`, directory tables, and property lists. The only subprocess that a query starts is `/bin/launchctl`, for the `Win32_Service` class. The `Win32_Process` `Create` method also starts the program that the caller names.

The local backend serves 30 classes in the `root\cimv2` namespace. Twenty-six classes have a provider. Four classes exist but return no instances.

A request for a node that is not the local machine, or with `/USER`, goes to the WinRM service on that node over WS-Management. [Remote nodes](#remote-nodes) lists the requirements and the limits.

The macOS build does not support these operations:

- `SET`, `CREATE`, and `DELETE`. They return `Not supported` (0x8004100C). This applies to the local machine and to remote nodes.
- `ASSOC`. It returns `Not supported` (0x8004100C). This applies to the local machine and to remote nodes.
- On the local machine, a namespace other than `root\cimv2`. It returns `Invalid namespace` (0x8004100E). A remote node receives the namespace in the resource URI and reports `Invalid namespace` for a namespace that it does not have.
- An object path that omits a key property, or uses the `=@` form for a class that is not a singleton. It returns `Invalid object path` (0x8004103A). For a remote node, this check applies to the classes in the embedded schema.

The local machine is `.`, `localhost`, `127.0.0.1`, `::1`, and the host name of the machine, with or without its domain suffix.

Full instance paths such as `\\.\root\cimv2:Win32_OperatingSystem=@` and paths using the local host name are validated before class lookup and key selection. A full path cannot bypass the local-node or `root\cimv2` restrictions. Namespace-qualified paths preserve colons inside quoted key values.

No macOS package or signed release exists. Build the program from source.

## Build

Use Rust 1.98.1 or later, as declared by `rust-version` in `Cargo.toml`. The repository does not pin a toolchain with `rust-toolchain.toml`. The build needs the Xcode Command Line Tools. It does not need Homebrew or pkg-config. See [Getting started](macos/getting-started.md) for prerequisites and optional installation.

```sh
cargo build --locked --release
```

The executable is `target/release/wmic`. The build links the system `libxml2` and `libxslt` libraries and the IOKit, CoreFoundation, and SystemConfiguration frameworks.

The build was verified on an Apple Silicon Mac mini that runs macOS 26.6.2. The CI workflow builds and tests on `macos-15` and `macos-26`. Both runners use arm64. Intel Macs and macOS versions before 15 are unverified.

## Use

Run `wmic` with the same commands as classic WMIC.

```sh
wmic os get Caption,Version
wmic os where "Caption like '%macOS%'" get Caption /format:csv
wmic process where "ProcessId=$$" call GetOwner
```

Run `wmic` without arguments to open the interactive prompt. Use `/INTERACTIVE:ON` to turn on confirmation prompts. The prompts use the Windows text.

### Shell quoting

The shell removes quotes before `wmic` starts. `wmic` rebuilds a Windows command line from the arguments it receives.

- `wmic` puts quotes around each argument that contains white space and is not already one token. The argument then stays one token.
- For an argument that starts with `/` and contains `:`, `wmic` keeps the `/name:` part as it is and quotes only the value. The argument `/output:/tmp/a b.txt` stays a switch.
- `wmic` keeps an empty argument as `""`.
- An argument that contains white space and both quote characters keeps escape backslashes in its value. Avoid that form.
- `wmic` does not restore quotes that the shell removed. The command `wmic /node:"a,b" os get Caption` passes `/node:a,b`. WMIC reads two nodes.
- To pass quotes to WMIC, put the whole argument in single quotes. The command `wmic '/node:"a,b"' os get Caption` passes one node, `a,b`.
- `wmic` converts bytes that are not valid UTF-8 to U+FFFD.

### Exit status

A WMIC status has 32 bits. A POSIX exit code has 8 bits. The macOS build maps the status as follows.

| WMIC status | Process result |
| --- | --- |
| `0` | Exit code 0. |
| `0xC000013A` (cancelled) | The process ends by signal SIGINT. Shells report 130. |
| Any other value | Exit code equal to the low byte of the status. A low byte of 0 becomes 1. |

Examples: `0x80041010` exits with 16. `0x80041002` exits with 2. `0x80070005` exits with 5. `0x80041000` exits with 1. The exit code does not hold the full 32-bit status.

### Output and encodings

| Destination | Encoding and line endings |
| --- | --- |
| Standard output and standard error | UTF-8. Line endings are LF. A lone CR stays unchanged. |
| `/OUTPUT` file, and a new `/APPEND` file | UTF-16LE with a byte order mark. Line endings are CRLF. |
| `/APPEND` to an existing file that starts with a UTF-16LE byte order mark | UTF-16LE. |
| `/APPEND` to an existing file that starts with a UTF-8 byte order mark | UTF-16LE append, retaining the original prefix/content; no transcoding of existing bytes. |
| `/APPEND` to an existing file that does not start with a byte order mark | UTF-8 with CRLF line endings. |
| `/RECORD` file | UTF-8. |
| `/OUTPUT:CLIPBOARD` and `/APPEND:CLIPBOARD` | Text sent to `/usr/bin/pbcopy` with LF line endings. |

The clipboard destination needs a graphical session. If `pbcopy` fails, `wmic` reports `Clipboard unavailable`.

### Console

- Ctrl+C and Ctrl+\ cancel the running command. A second Ctrl+C or Ctrl+\, or SIGTERM, restores the terminal settings and ends the process by that signal. An inherited ignored SIGINT stays ignored.
- The message `Hit any key to break the cycle` and the key check work when `/EVERY` runs on a foreground terminal.
- The Windows behavior that consumes one queued key after a query is not reproduced.
- Password prompts do not echo input when standard input is a foreground terminal.

### Formats and stylesheets

All built-in format names, aliases, parameters, format chains, and `/TRANSLATE` tables render in Rust code without an XSL engine. Their output matches the Windows captures that the tests compare against.

A custom `.xsl` file runs through the system `libxslt`. The engine applies these rules:

- The stylesheet cannot write files, create directories, or use the network.
- A stylesheet that declares a `script` element in the `urn:schemas-microsoft-com:xslt` namespace is refused. The status is 0xACB2. The message is `Invalid XSL format (or) file name. Script extensions are not supported on this platform.`
- A stylesheet with a document type declaration is refused with status 0xACB2 and the message `Invalid XSL format (or) file name.`
- The result is UTF-8 even when the stylesheet declares another encoding.
- `libxslt` writes an empty element as `<a/>`. MSXML6 writes `<a />` or `<a></a>`, depending on how the input wrote the element. Custom stylesheet output can differ from Windows in this respect.

## Remote nodes

A request for another node goes to the WinRM service on that node. Classic WMIC uses DCOM for remote WMI. The macOS build cannot speak DCOM. Remote WMI from macOS needs WinRM on the target. The Windows build is unchanged and still uses DCOM.

```sh
wmic /node:server01 /user:CONTOSO\admin os get Caption
```

### When a request is remote

- A request is remote when `/NODE` names a machine other than the local machine. [Scope](#scope) lists the local names.
- A request for the local machine with `/USER` is refused with `User credentials cannot be used for local connections` (0x80041064), as on Windows. When `WMIC_WSMAN_PORT` is set, such a request goes to WinRM on that port instead, which serves SSH tunnels to a remote listener.
- A `/NODE` without `/USER` sends an empty user name and an empty password. The build does not use the macOS login. Give `/USER` for each remote command.

### Requirements

- The target has WinRM enabled. Run `Enable-PSRemoting` or `winrm quickconfig` on the target.
- The build sends WS-Management over HTTP to TCP port 5985, path `/wsman`. It connects directly and does not use an HTTP proxy.
- Authentication is NTLMv2 inside the Negotiate scheme. After the logon, the build seals every SOAP message with the NTLM session key. Sealing encrypts and signs the message. This works against the Windows default, `AllowUnencrypted=false` with the Negotiate scheme enabled.
- The build ends the logon when the server does not grant sealing. It sends no SOAP message in plain text.
- The password does not cross the network. NTLMv2 sends a response that the build computes from it.
- Not implemented: Basic, Kerberos, CredSSP, certificate authentication, and HTTPS (port 5986). A target that allows only these methods is not supported.
- The connect timeout is 5 seconds. The read and write timeout is 90 seconds, longer than the 60 second operation timeout the request declares. A request that the server answers with a timeout fault is sent again until it completes or Ctrl+C ends it.

### Credentials and connection settings

| Input | Effect |
| --- | --- |
| `/USER:DOMAIN\name` | Logon as `name` in `DOMAIN`. |
| `/USER:name /AUTHORITY:NTLMDOMAIN:domain` | Logon as `name` in `domain`. A `DOMAIN\` prefix in `/USER` takes precedence. Other `/AUTHORITY` forms are ignored. |
| `/PASSWORD:text` | The password. The shell passes it to `wmic` as an argument, so other users on the Mac can read it in the process list. |
| `/USER` without `/PASSWORD` | `wmic` writes `Enter the password :` to standard error and reads one line from standard input. The input is not echoed when standard input is a terminal. A script can pipe the password. An empty line sends an empty password. |
| `/AUTHLEVEL`, `/IMPLEVEL`, `/PRIVILEGES`, `/LOCALE` | Accepted. They have no effect on WinRM. Every request uses the locale `en-US`. |
| `/FAILFAST:on` | Before the query, checks that a TCP connection to the listener port opens. A number sets the timeout in milliseconds. The default is 5000. On failure the command reports `RPC Server not available.` (0xABE3). |
| `WMIC_WSMAN_PORT` | An environment variable. Its value replaces port 5985 for all nodes. Use it for a non-standard listener or an SSH tunnel. A value that is not a port number from 1 to 65535 reports `Invalid parameter` (0x80041008). |

```sh
printf '%s\n' "$PASSWORD" | wmic /node:server01 /user:CONTOSO\admin os get Caption
```

### What works

- Alias queries and WQL queries: `GET`, `LIST`, `WHERE`, and property lists.
- `PATH` with an object path that has keys. An instance that does not exist returns no objects and no error.
- `CLASS` and `PATH <class> GET /?`. For a class in the embedded schema, the definition comes from the schema and the build sends nothing. For another class, see the list below.
- `CALL` on static methods such as `Win32_Process.Create`, and on instance methods such as `GetOwner`. For an instance method, the build first selects the instances, then calls the method on each one. The build does not repeat a method call.
- The confirmation prompts of `/INTERACTIVE:ON`. They use the Windows text, with the object path of the instance.
- Every output format.
- The alias set is the Windows set. An alias without a macOS provider, such as `printer`, works against a remote node.
- An enumeration reads 32 instances per request. Ctrl+C between requests ends the enumeration, releases it on the server, and keeps the instances received so far. A failed request also keeps them.

For a class outside the embedded schema (30 classes, see [Supported classes](#supported-classes)):

- Queries work. The build types every property value as a string.
- `CLASS` and `/?` read the first instance of the class. They list the property names with the type string and no methods. A class without an instance lists no properties.
- `__SUPERCLASS` is NULL and `__DYNASTY` is the class name.
- `CALL` sends the arguments named in the command as text. The call is static when the command has no object path and no `WHERE` clause.

For a class in the schema, a method that the class does not have returns `<name> - Invalid alias verb.` (0xABFD) and sends nothing.

### What does not work

`SET`, `CREATE`, `DELETE`, and `ASSOC` return `Not supported` (0x8004100C). The build sends nothing to the node for them.

### Data fidelity

- The embedded Windows class schema gives each value its type. A value that does not fit its declared type stays as text.
- A datetime converts from the WS-Management ISO form to the CIM form. The build keeps the wall-clock fields and the offset of the remote machine. It does not convert to UTC. A duration with days and time fields converts to a CIM interval.
- A reference property becomes an object path. An embedded object becomes a nested record.
- WinRM does not send object paths or the ten `__*` system properties. The build creates them from the endpoint reference and the schema: `__PATH`, `__RELPATH`, `__NAMESPACE`, `__SERVER`, `__CLASS`, `__SUPERCLASS`, `__DYNASTY`, `__DERIVATION`, `__GENUS`, and `__PROPERTY_COUNT`.
- The server part of an object path and `__SERVER` carry the computer name that the remote machine reports during authentication, as on Windows. When the exchange carries no name, the node name as typed, in upper case, is used.
- The `Node` column of the CSV format and the `Node - ` line of an error show the node as typed, as on Windows.
- The default namespace is `root\cimv2`. `/NAMESPACE` sets another namespace through the resource URI.

### Errors

| Case | Message | Status | Exit code |
| --- | --- | --- | ---: |
| Unknown class in an alias or WQL query | `Invalid class` | 0x80041010 | 16 |
| Unknown class in `PATH` | `Not found` | 0x80041002 | 2 |
| Unknown property | `Invalid query` | 0x80041017 | 23 |
| Unknown namespace | `Invalid namespace` | 0x8004100E | 14 |
| The server refuses the logon | `Access denied` | 0x80070005 | 5 |
| WMI refuses the operation | `Access denied` | 0x80041003 | 3 |
| The connection fails: refused, name not found, or no answer in 5 seconds | `The RPC server is unavailable.` | 0x800706BA | 186 |
| `SET`, `CREATE`, `DELETE`, `ASSOC` | `Not supported` | 0x8004100C | 12 |
| A fault with another WMI status, for example an invalid parameter or an invalid method | The text of that status | The status | Low byte of the status |
| A fault without a WMI status, an HTTP error, or a reply that is not SOAP | `Generic failure` | 0x80041001 | 1 |

- The connection failure keeps the text of classic WMIC so that scripts behave the same.
- The exit codes follow the mapping in [Exit status](#exit-status).
- A WMI status that has no WMIC text shows the text from the server. For `Generic failure`, the server text is kept in the error record and is not printed.

### Several nodes

`/NODE:a,b` runs the command for each node in turn. The build chooses the backend for each node. A local name stays on the local backend unless the command has `/USER`. With `/USER`, every node goes to WinRM, including the local names. Each node prints its own result. A failure on one node does not stop the next node. The exit status is the status of the last node that failed.

Requests to the same host, port, and account share one authenticated connection in one `wmic` process. A request that cannot change state repeats once on a new connection when a reused connection fails. A method call never repeats.

### Diagnose a listener

The example `wsman-probe` is not part of the `wmic` executable. It does not run on Windows.

```sh
cargo run --example wsman-probe -- HOST PORT [DOMAIN\]USER
cargo run --example wsman-probe -- --challenge HOST PORT
```

The first form reads the password from standard input, authenticates with NTLM, sends an Identify request, and prints the decrypted reply. The second form sends only the NTLM negotiate message and prints the parsed challenge. It uses no credentials.

## Supported classes

The bundled alias set is the Windows set: 81 aliases and 426 views. Help, `alias list`, `CONTEXT`, and `/?` behave as on Windows. Thirty aliases map to a class that this build serves. An alias for any other class returns `Invalid class` (0x80041010) when it is queried for the local machine. The bundled alias store answers `ALIAS` queries for the local node. The commands `path` and `class` with an unknown class name return `Not found` (0x80041002). A property name that the class does not have returns `Invalid query` (0x80041017).

Every property of a class is present in every instance. A value that macOS cannot provide is NULL. The column "Always NULL" lists notable properties that the provider never sets. Other properties can also be NULL when the source has no value.

Run the command as root to read the values that the "Privileges" column names.

| Alias | Class | Source and instances | Always NULL (notable) | Privileges |
| --- | --- | --- | --- | --- |
| `OS` | `Win32_OperatingSystem` | sysctl (`kern.osproductversion`, `kern.osversion`, `hw.memsize`, `vm.swapusage`, `kern.boottime`), Mach `host_statistics64`, `statfs`, utmpx login records. One instance. | `SerialNumber`, `OperatingSystemSKU`, `BuildType`, `CountryCode`, `CSDVersion`, `Organization`, `Description`, `SuiteMask`, `DataExecutionPrevention_*` | Methods need root. |
| `CPU` | `Win32_Processor` | sysctl (`machdep.cpu.*`, `hw.*`), IOKit device tree for the clock speed, Mach `host_statistics` for `LoadPercentage`. One instance. | `ProcessorId`, `SerialNumber`, `Family`, `Stepping`, `Revision`, `Version`, `SocketDesignation`, `UniqueId`, `PartNumber`, `ExtClock`, `VoltageCaps`, `PNPDeviceID` | None. |
| `ComputerSystem` | `Win32_ComputerSystem` | sysctl, SystemConfiguration (names), IOKit platform expert. One instance. | `DomainRole`, `PCSystemTypeEx`, `ChassisSKUNumber`, `BootStatus`, `OEMStringArray`, `InstallDate`, `ChassisBootupState`, `AutomaticManagedPagefile` | None. |
| `CSProduct` | `Win32_ComputerSystemProduct` | IOKit platform expert and device tree. One instance. | None are always NULL. `IdentifyingNumber`, `UUID`, and `SKUNumber` are NULL when IOKit has no value. | None. |
| `BIOS` | `Win32_BIOS` | IOKit device tree (`system-firmware-version`). One instance. | `ReleaseDate`, `BiosCharacteristics`, `SMBIOSMajorVersion`, `SMBIOSMinorVersion`, `SystemBiosMajorVersion`, `SystemBiosMinorVersion`, `BuildNumber`, `CurrentLanguage`, `ListOfLanguages` | None. |
| `BaseBoard` | `Win32_BaseBoard` | IOKit platform expert. One instance. | `Model`, `PartNumber`, `SKU`, `PoweredOn`, `HotSwappable`, `Removable`, `Replaceable`, `ConfigOptions` | None. |
| `SystemEnclosure` | `Win32_SystemEnclosure` | IOKit. `ChassisTypes` comes from the product name or model identifier prefix. One instance. | `Model`, `PartNumber`, `SKU`, `Version`, `SMBIOSAssetTag`, `SecurityStatus`, `TypeDescriptions` | None. |
| `MemoryChip` | `Win32_PhysicalMemory` | sysctl `hw.memsize` and the IOKit device tree (`dram-vendor`). One instance for all memory. | `Speed`, `PartNumber`, `SerialNumber`, `ConfiguredClockSpeed`, `MemoryType`, `SMBIOSMemoryType`, `TypeDetail`, `MinVoltage`, `MaxVoltage`, `ConfiguredVoltage` | None. |
| `MemPhysical` | `Win32_PhysicalMemoryArray` | sysctl `hw.memsize`. One instance. | `Manufacturer`, `Model`, `PartNumber`, `SerialNumber`, `SKU`, `Version`, `Status` | None. |
| `TimeZone` | `Win32_TimeZone` | The `/etc/localtime` link and `localtime_r`. One instance. | `SettingID`, `StandardDay`, `StandardMonth`, `StandardHour`, `DaylightDay`, `DaylightMonth`, `DaylightHour`, and the other transition fields | None. |
| `Environment` | `Win32_Environment` | The environment variables of the `wmic` process. One instance per variable. | `InstallDate` | None. |
| `PageFile` | `Win32_PageFileUsage` | sysctl `vm.swapusage`. No instance when the swap size is 0. | `Status`, `InstallDate` | None. |
| `LOGON` | `Win32_LogonSession` | utmpx `USER_PROCESS` records. One instance per record. | `AuthenticationPackage`, `Caption`, `Description`, `Name`, `Status`, `InstallDate` | None. |
| `Process` | `Win32_Process` | sysctl `kinfo_proc`, libproc (`proc_pidpath`, `proc_name`, `proc_pidinfo`), sysctl `KERN_PROCARGS2`. One instance per process. | `SessionId`, `HandleCount`, `Status`, `TerminationDate`, `PrivatePageCount`, `PeakWorkingSetSize`, `PageFileUsage`, `ReadOperationCount`, `WriteOperationCount`, `ReadTransferCount`, `WriteTransferCount` | `CommandLine`, `ThreadCount`, `WorkingSetSize`, `VirtualSize`, `PageFaults`, `KernelModeTime`, and `UserModeTime` of another user's process are NULL unless root. |
| `Service` | `Win32_Service` | `launchctl print` and `launchctl print-disabled` for the `system` domain and the `gui/<uid>` domain of the user, and the job property lists in the LaunchDaemons and LaunchAgents directories. One instance per label. Each `launchctl` run has a deadline of 30 seconds. | `Description`, `InstallDate` | Start, stop, and start-mode changes in the `system` domain need root. Under `sudo`, the user domain is the invoking user's domain (`SUDO_UID`). |
| `LogicalDisk` | `Win32_LogicalDisk` | `getfsstat`, `getattrlist`, `pathconf`. One instance for `/` and for each mount under `/Volumes`. A mount that macOS hides from browsing is not listed. | `BlockSize`, `NumberOfBlocks`, `PNPDeviceID`, `ErrorMethodology`, `Purpose`, `StatusInfo`, `InstallDate` | None. |
| `Volume` | `Win32_Volume` | `getfsstat`, `getattrlist`, `pathconf`. One instance per mounted device, except `devfs` and `autofs`. | `DriveLetter`, `Access`, `Availability`, `IndexingEnabled`, `Status`, `Description`, `PNPDeviceID` | None. |
| `DiskDrive` | `Win32_DiskDrive` | IOKit registry (`IOMedia`, `IOBlockStorageDevice`). Disk images and synthesized APFS containers are not listed. | `SectorsPerTrack`, `TotalCylinders`, `TotalHeads`, `TotalTracks`, `TracksPerCylinder`, `Signature`, `SCSIBus`, `SCSILogicalUnit`, `SCSIPort`, `SCSITargetId`, `PNPDeviceID`, `Status`, `Availability` | None. |
| `Partition` | `Win32_DiskPartition` | IOKit registry. One instance per partition-scheme child of a listed disk. | `BootPartition`, `HiddenSectors`, `RewritePartition`, `Access`, `Status`, `Availability`, `PNPDeviceID` | None. |
| `NIC` | `Win32_NetworkAdapter` | `getifaddrs`, IOKit (`IONetworkInterface`, `IONetworkController`), SystemConfiguration. Interfaces named `en<N>`, `bridge<N>`, and `vmnet<N>`. | `PermanentAddress`, `GUID`, `MaxSpeed`, `NetworkAddresses`, `AutoSense`, `PNPDeviceID`, `Status`, `InstallDate` | None. |
| `NICConfig` | `Win32_NetworkAdapterConfiguration` | `getifaddrs` and the SystemConfiguration dynamic store. The same interfaces as `NIC`. | `SettingID`, `IPConnectionMetric`, `TcpipNetbiosOptions`, `WINSPrimaryServer`, `WINSSecondaryServer`, `DefaultTTL`, `TcpWindowSize`, and the other TCP/IP tuning properties | None. |
| `UserAccount` | `Win32_UserAccount` | `getpwent`. User 0 and users with an id from 500 to 2147483647. Names that start with `_` are not listed. | `SID`, `Disabled`, `PasswordChangeable`, `PasswordExpires`, `PasswordRequired`, `Description`, `InstallDate` | None. |
| `Group` | `Win32_Group` | `getgrent`. Names that start with `_` or `com.apple.` are not listed. | `SID`, `Description`, `InstallDate` | None. |
| `Startup` | `Win32_StartupCommand` | Property lists in `/Library/LaunchAgents` and `~/Library/LaunchAgents` that launchd starts at login (`RunAtLoad` or `KeepAlive`) and that are not disabled. | `SettingID`, `UserSID` | None. |
| `Product` | `Win32_Product` | `Info.plist` of the application bundles in `/Applications` and `~/Applications`. A bundle without `CFBundleIdentifier` uses its path as `IdentifyingNumber`. | `Vendor`, `InstallSource`, `Language`, `PackageCode`, `ProductID`, `RegCompany`, `RegOwner`, `URLInfoAbout`, `HelpLink` | None. |
| `QFE` | `Win32_QuickFixEngineering` | `/Library/Receipts/InstallHistory.plist`. Only operating-system update entries are listed. One instance per `HotFixID`; a repeated entry keeps the newest date. | `Caption`, `Name`, `InstalledBy`, `FixComments`, `Status`, `InstallDate`, `ServicePackInEffect` | None. |
| `Temperature` | `Win32_TemperatureProbe` | No instances. | Not applicable. | None. |
| `Voltage` | `Win32_VoltageProbe` | No instances. | Not applicable. | None. |
| `CDROM` | `Win32_CDROMDrive` | No instances. | Not applicable. | None. |
| `TapeDrive` | `Win32_TapeDrive` | No instances. | Not applicable. | None. |

### Value mapping

| Property | Value on macOS |
| --- | --- |
| `Win32_OperatingSystem.Caption` | `macOS 26.6.2` |
| `Win32_OperatingSystem.Version` | `26.6.2` |
| `Win32_OperatingSystem.BuildNumber` | `25G83` (the `kern.osversion` value) |
| `Win32_OperatingSystem.OSArchitecture` | `ARM 64-bit Processor` on arm64. `64-bit` on x86_64. |
| `Win32_ComputerSystem.SystemType` | `ARM64-based PC` on arm64. `x64-based PC` on x86_64. |
| `Win32_ComputerSystem.Name` | The Bonjour local host name. The case is kept. |
| Object paths, `__SERVER`, `Win32_Process.CSName` | The first label of the host name, in upper case. |
| `Win32_LogicalDisk.DeviceID` | The mount point, for example `/`. There are no drive letters. |
| `Win32_Volume.DeviceID` | The device that the file system is mounted from, for example `/dev/disk3s1s1`. |
| `Win32_Service.Name` | The launchd label. |
| `Win32_Service.State` | `Running` when launchd reports a process id. Otherwise `Stopped`. NULL when launchd gives no runtime data. |
| `Win32_Service.StartMode` | `Disabled` when launchd disables the job. `Auto` when the job sets `RunAtLoad` or `KeepAlive`. Otherwise `Manual`. |
| `Win32_Process.Handle` | The process id, as text. |
| `Win32_DiskDrive.DeviceID` | `/dev/diskN`. |
| `Win32_DiskPartition.DeviceID` | `Disk #N, Partition #M`. |
| `Win32_NetworkAdapter.Index`, `Win32_NetworkAdapterConfiguration.Index` | The kernel interface index, the same value as `InterfaceIndex`. |
| Date and time properties | CIM datetime text. The offset is the local UTC offset at that instant. |

## Methods

The providers implement these methods. The list matches `implemented_methods` in the code. Every other method in the class schema returns `Not supported` (0x8004100C).

**Service control is experimental.** `StartService`, `StopService`, and `ChangeStartMode` are implemented and have mocked command-construction tests, but their successful launchd effects are unverified. Support requires a uniquely named, test-owned LaunchAgent lifecycle that verifies bootstrap/start, stop including KeepAlive, disable/enable, subsequent inventory state, and cleanup. The ordinary test suite does not change real launchd jobs.

| Class | Method | Action | `ReturnValue` |
| --- | --- | --- | --- |
| `Win32_Process` | `Create` | Starts the program in `CommandLine` with `posix_spawnp`. No shell runs. The new process starts in a new session. Its standard streams use `/dev/null`. `CurrentDirectory` sets the working directory. A relative program path resolves against the working directory of `wmic`. The out parameter `ProcessId` holds the new process id. `wmic` reaps the child when it exits. | 0 success. 21 empty command line. 9 program or directory not found. 2 access denied. 8 other failure. |
| `Win32_Process` | `Terminate` | Sends SIGKILL. The `Reason` argument is not used. | 0 success. 3 process id 1 or lower. 2 permission denied. 21 no such process. 8 other failure. |
| `Win32_Process` | `GetOwner` | Returns the account name of the process owner in `User`. Returns the upper-case host label in `Domain`. | 0 success. 21 no such process. 8 no account name. |
| `Win32_Process` | `SetPriority` | Calls `setpriority`. The Windows priority classes map to nice values: 64 to 19, 16384 to 10, 32 to 0, 32768 to -5, 128 to -10, 256 to -20. | 0 success. 21 unknown class or no such process. 3 invalid process id. 2 the system refuses the change. 8 other failure. |
| `Win32_Service` | `StartService` | Runs `launchctl kickstart` for a loaded job. Runs `launchctl bootstrap` with the job property list for a job that is not loaded. | 0 success. 10 already running. 14 disabled and not loaded. 2 access denied. 6 and 8 from `launchctl` failures. 21 no such label. |
| `Win32_Service` | `StopService` | Runs `launchctl bootout` for a job with `KeepAlive`. Runs `launchctl kill SIGTERM` for other jobs. | 0 success. 6 not running. 2 access denied. 8 other failure. 21 no such label. |
| `Win32_Service` | `ChangeStartMode` | Runs `launchctl disable` for `Disabled`. Runs `launchctl enable` for the values `Auto`, `Automatic`, `Boot`, `System`, and `Manual` on a disabled job. On an enabled job, launchd cannot switch between `Auto` and `Manual`. | 0 success, or the job already has the requested kind. 1 the job is enabled and has the other kind. 21 unknown mode or no such label. 2 access denied. 6 and 8 from `launchctl` failures. |
| `Win32_OperatingSystem` | `Reboot` | Runs `/sbin/shutdown -r now` without a shell, using the ordinary launchd shutdown path. | 0 request accepted. 5 privilege denied. 8 other failure. |
| `Win32_OperatingSystem` | `Shutdown` | Runs `/sbin/shutdown -h now` without a shell, using the ordinary launchd shutdown path. | 0 request accepted. 5 privilege denied. 8 other failure. |
| `Win32_OperatingSystem` | `Win32Shutdown`, `Win32ShutdownTracker` | Reads `Flags`. Values 1 and 8 use ordinary shutdown; 2 uses ordinary restart. Explicit force values 5 and 12 call `reboot(2)` with `RB_HALT`; 6 uses `RB_AUTOBOOT`. | 0 request accepted. 87 other flag values. 5 privilege denied. 8 other failure. |

Each method call is subject to the privilege rules in the class table. A call on a service in the `system` domain without root returns 2. The operating-system methods return 5 without root and do nothing.

The power implementation follows Apple's local `reboot(2)` and `shutdown(8)` manuals, recorded in the [shutdown fix evidence](research/evidence/2026-10-16/macos-shutdown-fix.json). Ordinary requests let `shutdown(8)` notify launchd; they never fall back to a forced operation. An OS permission error (`EPERM` or `EACCES`) returns 5. Other command start/wait failures, nonzero exits, and signal termination return 8. A zero result means the request was accepted, not that shutdown completed; the system may terminate this process before it returns an output record.

This is an administrative shutdown path. Apple's `shutdown(8)` manual describes SIGTERM followed by SIGKILL after an indeterminate interval, giving programs time to flush data and exit. It does not promise the Finder or Apple-menu shutdown interaction, unsaved-document prompts, or application cancellation. Explicit force bypasses that application termination sequence and may lose unsaved data; filesystem syncing remains enabled in the kernel. No real shutdown, restart, or forced kernel call has been exercised for acceptance.

## Known differences from Windows

- Windows concepts map to macOS concepts as listed in [Value mapping](#value-mapping). Programs that read a value as a drive letter, a Windows service name, or a Windows SID get macOS values or NULL.
- `Win32_Environment` lists the environment of the `wmic` process. It does not list user and system variables.
- `Win32_Service` lists launchd jobs. `ServiceType` is `Own Process`. `AcceptStop` is true. `AcceptPause` is false.
- Each `Win32_Service` query starts `/bin/launchctl` four times: `print` and `print-disabled`, each for the `system` domain and the user's `gui` domain. A method call reuses the inventory for 5 seconds.
- The local backend reads only the node and the namespace of a request. It does not read `/USER`, `/PASSWORD`, `/AUTHORITY`, `/LOCALE`, `/AUTHLEVEL`, `/IMPLEVEL`, or `/PRIVILEGES`. The remote backend reads the node, the namespace, `/USER`, `/PASSWORD`, and `/AUTHORITY`. [Remote nodes](#remote-nodes) lists the effect of the other switches.
- A remote node returns the values of the Windows host. The tables in [Supported classes](#supported-classes), [Value mapping](#value-mapping), and [Methods](#methods) describe the local machine only.
- `Win32_OperatingSystem.Locale` and `OSLanguage` come from the `LC_ALL`, `LC_MESSAGES`, and `LANG` environment variables. Only English locales have values.
- `Win32_Process.Create` ignores `ProcessStartupInformation`.
- The format and stylesheet differences are listed in [Formats and stylesheets](#formats-and-stylesheets).
- The console differences are listed in [Console](#console).

## Verification

The evidence for the macOS build differs from the Windows evidence in [compatibility.md](compatibility.md).

- No Microsoft WMIC exists for macOS. The project has no reference corpus for data values on macOS.
- Output formatting is checked against the captured Windows output in `tests/fixtures`. The shared renderer tests run on macOS.
- Data shape is checked against the Windows class schema. The file `assets/macos-schema.json` holds the classes, property names, types, and method signatures.
- Local data values are checked against the macOS sources on one machine, an Apple Silicon Mac mini that runs macOS 26.6.2.
- Remote node behavior is checked against WinRM reply fixtures in `tests/fixtures/wsman` and against two live Windows hosts. See [Remote node evidence](#remote-node-evidence).

The CI workflow defines a `macos` job for the runners `macos-15` and `macos-26`. The job runs `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo build --locked --release`, and `cargo test --locked`.

The test files below contain these `#[test]` functions in this revision.

| File | Tests | Runs on | Checks |
| --- | ---: | --- | --- |
| `tests/platform_unix.rs` | 22 | Unix | Argument quoting, switch values, exit-status mapping, node name, LF output, password redaction |
| `tests/formats_portable.rs` | 25 | All platforms | Built-in renderers against captured Windows output, wildcard gets |
| `tests/macos_backend.rs` | 25 | macOS | Class registry, selection, object paths, errors, namespace, node, prompts, writes |
| `tests/macos_processes.rs` | 47 | macOS | Process, service, account, startup, product, and update providers, and their methods |
| `tests/macos_storage_network.rs` | 6 | macOS | Logical disk, volume, disk drive, partition, network adapter, and configuration |
| `tests/macos_xslt.rs` | 17 | macOS | Custom stylesheets, parameters, refusals, write block, network and entity isolation |
| `tests/macos_cli.rs` | 12 | macOS | Built executable, real argv, UTF-8 streams, POSIX status, empty selections, spaced UTF-16 output/append files, redirected interactive recovery/EOF, credential redaction, full paths |
| `src/backend/macos/providers/mapping_tests.rs` | 4 | macOS | Fixed nonempty disk/partition/interface/DHCP/DNS snapshots with exact expected typed records |
| `src/backend/macos/providers/operating_system.rs` (unit tests) | 6 | macOS | All four power methods, force preservation, flag validation, fake privilege denial, and result shape through injected adapters |
| `src/backend/macos/sys/power_tests.rs` | 8 | macOS | Exact ordinary versus forced operation selection, zero action calls when privilege is denied, command status/error mapping, and no forced fallback |
| `tests/wsman_protocol.rs` | 51 | Unix (3 on macOS only) | Enumerate, pull, release, get, invoke, faults, typing, object paths, nested reference keys, routing, port validation, stale connections, timeouts |
| `tests/wsman_transport.rs` | 27 | Unix | NTLM vectors from MS-NLMP 4.2.4, loopback server with a real handshake, sealing, framing variants, chunked HTTP, unsealed-reply refusal, wrong password, closed connections |
| `src/backend/wsman/**` (unit tests) | 31 | Unix | Value conversion, fault precedence, parsing limits, path quoting, record building, envelopes, timeouts, the port variable |
| `src/platform/unix/console.rs` (unit tests) | 4 | Unix | Signal handler, inherited dispositions, terminal restore |
| `src/alias/predicate.rs` (unit tests) | 6 | All platforms | WHERE literal coercion and comparison types |
| Total | 291 | | |

The five `tests/macos_*.rs` files hold 107 of the 291 tests listed here. The remote-node suites run on every Unix platform. Unix and Windows-specific suites have explicit platform gates. At the earlier commit b2755a9, `cargo test --locked` reported 311 passed and 0 failed on the Mac mini, and 189 passed, 0 failed, 33 ignored on Windows.

The earlier follow-up passed 327 tests on Apple Silicon, including 12 executable tests and four fixed-snapshot mapping tests. [Review evidence](research/evidence/2026-10-16/macos-pr-review.json) records that baseline. The shutdown fix passes 340 tests: it adds 14 injected tests and replaces the prior native privilege/invalid-flag test, so all power behavior is tested through fakes, independently of the test runner's real user ID. [Shutdown fix evidence](research/evidence/2026-10-16/macos-shutdown-fix.json) records the fresh checks and remaining acceptance limits.

Windows CI now separately runs all 12 tests in `formats_msxml_parity` with `--ignored --test-threads=1`. This targeted invocation enables the captured/generated-value differential gate against MSXML6 without enabling unrelated ignored tests that need a Windows reference share. The macOS CLI suite launches the actual Cargo-built executable with bounded child lifetimes and uses only owned temporary files. It does not invoke power or service-control methods.

The tests that read the live system assert structure and internal consistency. Examples: the process table contains the test process, the root file system is a logical disk, volumes have unique devices, and network configurations follow the adapters.

Required system providers must return instances on the supported macOS runners. Disk/partition and adapter tests reject empty results, and every adapter requires a configuration. The four explicitly unpopulated hardware classes remain allowed to be empty; other optional providers can be empty when no corresponding data exists. Fixed input snapshots additionally verify disk ownership/geometry/type, physical and virtual links, IPv4/IPv6 addressing, DHCP, DNS, missing values, and disconnected interfaces independently of live hardware availability.

### Remote node evidence

The commands below ran on 2026-10-16 from the Mac mini against two Windows hosts through an SSH tunnel to the WinRM listener. One host runs Windows 11 Enterprise Evaluation (build 26200). The other host runs Windows Server 2025 Datacenter Evaluation (build 26100). Both are lab machines. The exit code of every command was read from the shell. For a command with a pipe, the exit code is the exit code of the pipeline.

| Host | Command | Result |
| --- | --- | --- |
| Windows 11 | `os get Caption,Version,BuildNumber,CSName,LastBootUpTime,FreePhysicalMemory` | One row. `LastBootUpTime` is in CIM form with the remote offset (`-240`). Exit 0. |
| Windows 11 | `os get Caption,Primary,MUILanguages,OSArchitecture /format:list` | `Primary` is `TRUE`. `MUILanguages` is an array. Exit 0. |
| Windows 11 | `cpu get Name,NumberOfCores,MaxClockSpeed` | One row. Exit 0. |
| Windows 11 | `logicaldisk get DeviceID,FileSystem,Size,FreeSpace` | One row for the `C:` drive. Exit 0. |
| Windows 11 | `process where "Name='lsass.exe'" get Name,ProcessId,ExecutablePath,CreationDate` | One row. `ExecutablePath` is empty for this protected process. Exit 0. |
| Windows 11 | `process get Name,ProcessId /format:csv` | CSV with a `Node` column that holds the node as typed. Exit 0. |
| Windows 11 | `os get __SERVER,__PATH,__NAMESPACE /format:list` | `__SERVER` and the server part of `__PATH` hold the remote computer name. `__NAMESPACE` is `ROOT\CIMV2`. Exit 0. |
| Windows 11 | `service where "StartMode='Auto'" get Name` | 67 lines of output, more than one page of 32 instances. Exit 0. |
| Windows 11 | `nicconfig where IPEnabled=TRUE get IPAddress,MACAddress,DHCPEnabled /format:list` | Two adapters. `IPAddress` is an array. `DHCPEnabled` is a Boolean. Exit 0. |
| Windows 11 | `path Win32_Nope get Name` | `Not found`, exit 2, no `Node - ` line. |
| Windows 11 | `os get Nope` | `Invalid query`, exit 23. |
| Windows 11 | `path Win32_OperatingSystem get "/?"` | The property help from the embedded schema. Exit 0. |
| Windows 11 | `process call create "cmd.exe /c exit 0"` | `Method execution successful.` The out parameters hold a process id and `ReturnValue = 0`. Exit 0. |
| Windows 11 | `process where "Name='lsass.exe'" call getowner` | `Method execution successful.` `Domain` and `User` hold the owner. `ReturnValue = 0`. Exit 0. |
| Windows 11 | `path Win32_LogicalDiskToPartition get __RELPATH /format:list` | The association keys are quoted object paths of the partition and the disk. Exit 0. |
| Windows 11 | `/namespace:\root
osuchns path Win32_OperatingSystem get Caption` | `Invalid namespace`, exit 14. |
| Windows Server | `os get Caption,Version,CSName` | One row. Exit 0. |
| Windows Server | `service where "Name='CcmExec'" get Name,DisplayName,State,StartMode,ProcessId /format:csv` | One row. Exit 0. |
| Windows Server | `printer get Name` | One printer row. Exit 0. |
| Windows Server | `useraccount get Name,Domain,SID /format:csv` | CSV rows with SIDs. Exit 0. |
| Windows Server | `product get Name,Version /format:csv` | CSV rows of installed products. Exit 0. |

The same commands run through the Windows build against `/node:127.0.0.1` print the typed address in the `Node` column and the computer name in `__SERVER`, which is what the macOS build prints.

## Not verified

- Intel Macs. The code has separate values for x86_64. They have not run.
- macOS versions before 15. Manual runs on macOS 15 are unverified.
- Real ordinary or forced `Win32_OperatingSystem` shutdown/restart effects. Injected tests cover the privileged route and result mapping, but no native power action or GUI shutdown interaction has been exercised.
- Experimental service control. The `StartService`, `StopService`, and `ChangeStartMode` methods have no real test-owned launchd lifecycle acceptance. Only parsing and command construction have tests.
- Remote nodes, in addition to the commands in [Remote node evidence](#remote-node-evidence):
  - Kerberos, CredSSP, Basic, certificate authentication, and HTTPS. The build does not implement them.
  - Hosts that require unencrypted traffic or Basic authentication. The build does not support them.
  - Servers with a language other than English. Both live hosts use English. Every request asks for the locale `en-US`.
  - HTTP proxies. The build does not use them.
  - IPv6 literals in `/NODE`. The code brackets an IPv6 literal in the URL. No test or live run uses one.
  - Large enumerations. The longest output that a live run counted had 67 lines.
  - Enumeration context expiry. Only fixture replies test the restart after an expired context.
  - `SET`, `CREATE`, and `DELETE` over WinRM. The build does not implement them.
  - More than one node in one command. The code runs the nodes in turn through the dispatcher. No live run uses a node list.
- The clipboard outside a graphical session.
- The `sudo` path that selects the invoking user's launchd domain. A unit test covers the selection only.
- Arguments that are not valid UTF-8.
- The alias `UPS` and the class `Win32_UninterruptiblePowerSupply`. The class is absent from the schema source.
- The aliases `Printer`, `Share`, `SoundDev`, `DesktopMonitor`, and every other bundled alias that the class table does not list. These aliases have no provider. A query for the local machine returns `Invalid class`.
- The `libxslt` serialization of empty elements. The test suite does not assert it.
- The `/EVERY` key check and the password prompt on a terminal. The test suite does not drive a terminal.
