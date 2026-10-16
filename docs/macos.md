# macOS build

This document describes the macOS build of `wmic`. It states what the build does, how to use it, and what evidence supports each claim. The evidence differs from the Windows build. See [Verification](#verification) and [Not verified](#not-verified).

## Scope

The macOS build runs the same parser, help, alias set, formats, and session logic as the Windows build. It does not use WMI. A backend answers each query from macOS system interfaces: sysctl, Mach, libproc, IOKit, Core Foundation, SystemConfiguration, `getfsstat`, `getifaddrs`, directory tables, and property lists. The only subprocess that a query starts is `/bin/launchctl`, for the `Win32_Service` class. The `Win32_Process` `Create` method also starts the program that the caller names.

The backend serves 30 classes in the `root\cimv2` namespace. Twenty-six classes have a provider. Four classes exist but return no instances.

The macOS build does not support these operations:

- `SET`, `CREATE`, and `DELETE`. They return `Not supported` (0x8004100C).
- `ASSOC`. It returns `Not supported` (0x8004100C).
- A node that is not the local machine. It returns `The RPC server is unavailable.` (0x800706BA).
- A namespace other than `root\cimv2`. It returns `Invalid namespace` (0x8004100E).
- An object path that omits a key property, or uses the `=@` form for a class that is not a singleton. It returns `Invalid object path` (0x8004103A).

The local machine is `.`, `localhost`, `127.0.0.1`, `::1`, and the host name of the machine, with or without its domain suffix.

No macOS package or signed release exists. Build the program from source.

## Build

The repository pins Rust 1.98.1 in `rust-toolchain.toml`. The build needs the Xcode Command Line Tools. It does not need Homebrew or pkg-config.

```sh
cargo build --release
```

The executable is `target/release/wmic`. The build links the system `libxml2` and `libxslt` libraries and the IOKit, CoreFoundation, and SystemConfiguration frameworks.

The build was verified on an Apple Silicon Mac mini that runs macOS 26.6.2. The CI workflow builds and tests on `macos-15` and `macos-26`. Both runners use arm64. Intel Macs and macOS versions before 15 are unverified.

## Use

Run `wmic` with the same commands as classic WMIC.

```sh
wmic os get Caption,Version
wmic os where "Caption like '%macOS%'" get Caption /format:csv
wmic process where "Name='sleep'" call terminate
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

## Supported classes

The bundled alias set is the Windows set: 81 aliases and 426 views. Help, `alias list`, `CONTEXT`, and `/?` behave as on Windows. Thirty aliases map to a class that this build serves. An alias for any other class returns `Invalid class` (0x80041010) when it is queried. The bundled alias store answers `ALIAS` queries for the local node. The commands `path` and `class` with an unknown class name return `Not found` (0x80041002). A property name that the class does not have returns `Invalid query` (0x80041017).

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

| Class | Method | Action | `ReturnValue` |
| --- | --- | --- | --- |
| `Win32_Process` | `Create` | Starts the program in `CommandLine` with `posix_spawnp`. No shell runs. The new process starts in a new session. Its standard streams use `/dev/null`. `CurrentDirectory` sets the working directory. A relative program path resolves against the working directory of `wmic`. The out parameter `ProcessId` holds the new process id. `wmic` reaps the child when it exits. | 0 success. 21 empty command line. 9 program or directory not found. 2 access denied. 8 other failure. |
| `Win32_Process` | `Terminate` | Sends SIGKILL. The `Reason` argument is not used. | 0 success. 3 process id 1 or lower. 2 permission denied. 21 no such process. 8 other failure. |
| `Win32_Process` | `GetOwner` | Returns the account name of the process owner in `User`. Returns the upper-case host label in `Domain`. | 0 success. 21 no such process. 8 no account name. |
| `Win32_Process` | `SetPriority` | Calls `setpriority`. The Windows priority classes map to nice values: 64 to 19, 16384 to 10, 32 to 0, 32768 to -5, 128 to -10, 256 to -20. | 0 success. 21 unknown class or no such process. 3 invalid process id. 2 the system refuses the change. 8 other failure. |
| `Win32_Service` | `StartService` | Runs `launchctl kickstart` for a loaded job. Runs `launchctl bootstrap` with the job property list for a job that is not loaded. | 0 success. 10 already running. 14 disabled and not loaded. 2 access denied. 6 and 8 from `launchctl` failures. 21 no such label. |
| `Win32_Service` | `StopService` | Runs `launchctl bootout` for a job with `KeepAlive`. Runs `launchctl kill SIGTERM` for other jobs. | 0 success. 6 not running. 2 access denied. 8 other failure. 21 no such label. |
| `Win32_Service` | `ChangeStartMode` | Runs `launchctl disable` for `Disabled`. Runs `launchctl enable` for the values `Auto`, `Automatic`, `Boot`, `System`, and `Manual` on a disabled job. On an enabled job, launchd cannot switch between `Auto` and `Manual`. | 0 success, or the job already has the requested kind. 1 the job is enabled and has the other kind. 21 unknown mode or no such label. 2 access denied. 6 and 8 from `launchctl` failures. |
| `Win32_OperatingSystem` | `Reboot` | Calls `reboot(2)` with the restart flag. | 0 success. 5 not root. 8 other failure. |
| `Win32_OperatingSystem` | `Shutdown` | Calls `reboot(2)` with the halt flag. | 0 success. 5 not root. 8 other failure. |
| `Win32_OperatingSystem` | `Win32Shutdown`, `Win32ShutdownTracker` | Reads `Flags`. The values 1 and 8 halt. The value 2 restarts. Bit 4 (force) is accepted with them. | 0 success. 87 other flag values. 5 not root. 8 other failure. |

Each method call is subject to the privilege rules in the class table. A call on a service in the `system` domain without root returns 2. The operating-system methods return 5 without root and do nothing.

## Known differences from Windows

- Windows concepts map to macOS concepts as listed in [Value mapping](#value-mapping). Programs that read a value as a drive letter, a Windows service name, or a Windows SID get macOS values or NULL.
- `Win32_Environment` lists the environment of the `wmic` process. It does not list user and system variables.
- `Win32_Service` lists launchd jobs. `ServiceType` is `Own Process`. `AcceptStop` is true. `AcceptPause` is false.
- Each `Win32_Service` query starts `/bin/launchctl` four times: `print` and `print-disabled`, each for the `system` domain and the user's `gui` domain. A method call reuses the inventory for 5 seconds.
- The backend reads only the node and the namespace of a request. It does not read `/USER`, `/PASSWORD`, `/AUTHORITY`, `/LOCALE`, `/AUTHLEVEL`, `/IMPLEVEL`, or `/PRIVILEGES`.
- `Win32_OperatingSystem.Locale` and `OSLanguage` come from the `LC_ALL`, `LC_MESSAGES`, and `LANG` environment variables. Only English locales have values.
- `Win32_Process.Create` ignores `ProcessStartupInformation`.
- The format and stylesheet differences are listed in [Formats and stylesheets](#formats-and-stylesheets).
- The console differences are listed in [Console](#console).

## Verification

The evidence for the macOS build differs from the Windows evidence in [compatibility.md](compatibility.md).

- No Microsoft WMIC exists for macOS. The project has no reference corpus for data values on macOS.
- Output formatting is checked against the captured Windows output in `tests/fixtures`. The shared renderer tests run on macOS.
- Data shape is checked against the Windows class schema. The file `assets/macos-schema.json` holds the classes, property names, types, and method signatures.
- Data values are checked against the macOS sources on one machine, an Apple Silicon Mac mini that runs macOS 26.6.2.

The CI workflow defines a `macos` job for the runners `macos-15` and `macos-26`. The job runs `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo build --locked --release`, and `cargo test --locked`.

The test files below contain these `#[test]` functions as of commit b2755a9.

| File | Tests | Runs on | Checks |
| --- | ---: | --- | --- |
| `tests/platform_unix.rs` | 22 | Unix | Argument quoting, switch values, exit-status mapping, node name, LF output, password redaction |
| `tests/formats_portable.rs` | 25 | All platforms | Built-in renderers against captured Windows output, wildcard gets |
| `tests/macos_backend.rs` | 25 | macOS | Class registry, selection, object paths, errors, namespace, node, prompts, writes |
| `tests/macos_processes.rs` | 48 | macOS | Process, service, account, startup, product, and update providers, and their methods |
| `tests/macos_storage_network.rs` | 6 | macOS | Logical disk, volume, disk drive, partition, network adapter, and configuration |
| `tests/macos_xslt.rs` | 17 | macOS | Custom stylesheets, parameters, refusals, write block, network and entity isolation |
| `src/platform/unix/console.rs` (unit tests) | 4 | Unix | Signal handler, inherited dispositions, terminal restore |
| `src/alias/predicate.rs` (unit tests) | 6 | All platforms | WHERE literal coercion and comparison types |
| Total | 153 | | |

The four `tests/macos_*.rs` files hold 96 of the 153 tests. The other files in `tests/` have no platform gate. They run in the same `cargo test` command. At commit b2755a9, `cargo test --locked` reported 311 passed and 0 failed on the Mac mini, and 189 passed, 0 failed, 33 ignored on Windows. The ignored tests need native Windows COM or a Windows reference share.

The tests that read the live system assert structure and internal consistency. Examples: the process table contains the test process, the root file system is a logical disk, volumes have unique devices, and network configurations follow the adapters.

## Not verified

- Intel Macs. The code has separate values for x86_64. They have not run.
- macOS versions before 15. Manual runs on macOS 15 are unverified.
- The root path of the `Win32_OperatingSystem` shutdown methods.
- Service control. The `StartService`, `StopService`, and `ChangeStartMode` methods were not run against a real launchd job during development. Only parsing and command construction have tests.
- Remote nodes.
- The clipboard outside a graphical session.
- The `sudo` path that selects the invoking user's launchd domain. A unit test covers the selection only.
- Arguments that are not valid UTF-8.
- The alias `UPS` and the class `Win32_UninterruptiblePowerSupply`. The class is absent from the schema source.
- The aliases `Printer`, `Share`, `SoundDev`, `DesktopMonitor`, and every other bundled alias that the class table does not list. These aliases have no provider. A query returns `Invalid class`.
- The `libxslt` serialization of empty elements. The test suite does not assert it.
- The `/EVERY` key check and the password prompt on a terminal. The test suite does not drive a terminal.
