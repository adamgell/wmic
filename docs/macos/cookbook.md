# macOS command cookbook

These examples use `wmic` on PATH. From a source checkout, substitute `./target/release/wmic`. The [getting-started guide](getting-started.md) covers building and installation.

The inventory queries below are read-only. The process-method section distinguishes its short-lived process creation example from queries. Service and power mutations are reference material only and are excluded from the runnable examples.

## Command shape

```text
wmic [global switches] <alias> [where <condition>] get <properties> [format switches]
wmic path <class-or-object-path> get <properties>
wmic <alias> [where <condition>] call <method> [arguments]
```

Use commas without shell spaces between selected properties, or quote the complete property list. Alias and property names are case-insensitive. WMIC may render columns in its own property order.

## OS, identity, and boot time

```sh
wmic os get Caption,Version,BuildNumber,OSArchitecture /value
wmic os get LastBootUpTime,LocalDateTime /value
wmic computersystem get Name,Manufacturer,Model,SystemType /value
wmic csproduct get Name,IdentifyingNumber,UUID /value
```

`Caption` is a macOS description. `BuildNumber` is the Darwin OS build identifier. `IdentifyingNumber` and `UUID` come from IOKit when available; `OS.SerialNumber` is not a substitute and is always NULL on this provider. Inventory exports containing device identity should be handled as machine inventory.

CIM datetime properties have the form `yyyymmddHHMMSS.mmmmmm±UUU`, where `UUU` is the UTC offset in minutes at that instant. They are strings, not Unix timestamps. For example, the final `-240` means UTC minus four hours. Do not drop the offset when converting a boot time.

## CPU and hardware

```sh
wmic cpu get Name,NumberOfCores,NumberOfLogicalProcessors,LoadPercentage
wmic bios get Manufacturer,SMBIOSBIOSVersion,Version /value
wmic baseboard get Manufacturer,Product,Version /value
wmic systemenclosure get Manufacturer,ChassisTypes,SerialNumber /value
```

The CPU provider returns one aggregate processor record. `LoadPercentage` is a sample from the macOS CPU statistics, not a Windows performance-counter history. Fields unavailable on this Mac are NULL. Windows terminology in property names does not imply that the underlying hardware has a PC BIOS or Windows chassis model.

### Memory

```sh
wmic computersystem get TotalPhysicalMemory /value
wmic os get TotalVisibleMemorySize,FreePhysicalMemory /value
wmic memorychip get BankLabel,Capacity,DeviceLocator /value
wmic memphysical get MaxCapacity,MaxCapacityEx,MemoryDevices /value
```

| Property | Unit or interpretation |
| --- | --- |
| `ComputerSystem.TotalPhysicalMemory` | Bytes |
| `OS.TotalVisibleMemorySize`, `OS.FreePhysicalMemory` | KiB (1024 bytes) |
| `MemoryChip.Capacity` | Bytes; one aggregate record for the machine's memory |
| `MemPhysical.MaxCapacity` | KiB |
| `MemPhysical.MaxCapacityEx` | Bytes |

`MemoryChip` does not enumerate individual removable DIMMs on this backend. A blank `Speed`, `PartNumber`, or `SerialNumber` is expected; see the [NULL property reference](../macos.md#supported-classes).

Inspect swap usage separately:

```sh
wmic pagefile get Name,AllocatedBaseSize,CurrentUsage
```

The pagefile capacity and usage fields use MiB. This class can have no instances when macOS has allocated no swap.

## Storage

Mounted, user-visible logical disks:

```sh
wmic logicaldisk get DeviceID,VolumeName,FileSystem,Size,FreeSpace
wmic logicaldisk where "DeviceID='/'" get DeviceID,FileSystem,Size,FreeSpace /value
```

`DeviceID` is a mount point such as `/` or `/Volumes/Example`, rather than `C:`. `Size` and `FreeSpace` are bytes. The logical-disk provider includes `/` and browsable mounts under `/Volumes`; it does not list every internal mount.

Inspect volumes, physical disks, and partition-scheme entries:

```sh
wmic volume get DeviceID,Name,FileSystem,Capacity,FreeSpace
wmic diskdrive get DeviceID,Model,InterfaceType,Size,Partitions
wmic partition get DeviceID,DiskIndex,Index,Size,StartingOffset
```

These are different views of storage. `Volume.DeviceID` is a mounted device path. `DiskDrive` comes from IOKit and excludes disk images and synthesized APFS containers. `Partition` lists partition-scheme children of those disks; it is not an APFS volume list. Use `DiskIndex` to relate a partition to `DiskDrive.Index`; `ASSOC` joins are unavailable.

## Networking

Inspect interface identity and link state:

```sh
wmic nic get Index,NetConnectionID,MACAddress,NetEnabled,PhysicalAdapter
```

Inspect configured addresses, routes, and DNS for interfaces with IP enabled:

```sh
wmic nicconfig where "IPEnabled=TRUE" get Index,IPAddress,IPSubnet,DefaultIPGateway,DNSServerSearchOrder /value
```

The adapter and configuration classes use the same kernel interface index. `NIC.Index` corresponds to `NICConfig.Index`; both also expose `InterfaceIndex`. The provider covers `en<N>`, `bridge<N>`, and `vmnet<N>` interfaces, rather than every macOS interface type.

`IPAddress`, `IPSubnet`, gateways, and DNS servers are arrays. `/value` displays the complete property without assuming it fits a table cell. An array can be empty, and a disconnected interface can have NULL configuration fields. `IPSubnet` follows the provider's address/subnet representation; do not assume every value is an IPv4 dotted mask.

The backend enumerates these records locally. `/NODE` is not a way to query another Mac or a remote Windows host:

```sh
wmic /node:localhost os get Caption
```

Other accepted local identities include `.`, `127.0.0.1`, `::1`, and this machine's hostname. See the [scope reference](../macos.md#scope).

## Processes

```sh
wmic process get ProcessId,ParentProcessId,Name
wmic process where "Name like '%Safari%'" get ProcessId,Name,ExecutablePath
wmic process where "ProcessId=$$" get ProcessId,Name,ParentProcessId /value
wmic process where "WorkingSetSize > 104857600 AND Name IS NOT NULL" get ProcessId,Name,WorkingSetSize
```

The shell expands `$$` to the calling shell's process ID. That example therefore targets your shell. `WorkingSetSize` uses bytes. Fields such as other users' `CommandLine` or task-memory statistics can be NULL without root; a filtered query may consequently omit those processes.

Ask who owns a specific process without changing it:

```sh
wmic process where "ProcessId=$$" call GetOwner
```

The output includes `ReturnValue`, `User`, and `Domain`. `Domain` is the host label used by this backend, not an Active Directory membership claim. See [method results](scripting.md#process-status-and-method-results) before scripting around it.

### Process creation and termination

This call starts a short-lived program that exits successfully:

```sh
wmic process call Create '"/usr/bin/true"'
```

`Create` returns a `ProcessId`. `/usr/bin/true` may already have exited when the command returns. `Create` starts the named program without a shell, detaches its session, and connects its standard streams to `/dev/null`. Shell operators such as `>`, `|`, and `&&` in its command-line string are not interpreted. The separate `CurrentDirectory` parameter changes the working directory; it does not invoke a shell either.

For a complete inspect/owner/terminate demonstration, use the [owned-process example](../../examples/macos/README.md#owned-process-lifecycle). It creates one child, selects only that PID, checks each method's `ReturnValue`, and cleans up its child. Do not select all processes named `sleep` or another common name for a termination example.

`Terminate` sends **SIGKILL**, not a graceful application quit request. `SetPriority` uses Windows priority-class numbers mapped to macOS nice values; raising priority may be denied. The exact mapping and return codes are in the [method reference](../macos.md#methods).

## Applications, startup, updates, and services

```sh
wmic product get Name,Version,InstallLocation
wmic startup get Name,Command,Location
wmic qfe get HotFixID,Description,InstalledOn
wmic service get Name,State,StartMode,ProcessId
```

- `Product` reads application-bundle metadata in `/Applications` and `~/Applications`. It is not a package receipt, Homebrew, App Store entitlement, or complete managed-software inventory. This macOS query does not invoke the Windows `Win32_Product` provider.
- `Startup` reads enabled launch-at-login property lists in the listed LaunchAgents locations. It does not enumerate every macOS login-item mechanism.
- `QFE` reads OS update entries from install history, keeping the newest entry for a repeated `HotFixID`. It is not a Windows KB catalog.
- `Service.Name` is a launchd label. The service query can take longer because it invokes `launchctl` for runtime and disabled-state information, with per-command deadlines.

Service mutation methods are **experimental**. A successful, test-owned LaunchAgent start/stop/mode/cleanup lifecycle has not been verified. These guides supply service inventory queries; they do not supply a runnable service mutation script. Keep the [service acceptance limit](../macos.md#methods) visible when evaluating those methods.

## Accounts, environment, and timezone

```sh
wmic useraccount get Name,FullName,Domain,LocalAccount
wmic group get Name,Domain
wmic timezone get Caption,StandardName,Bias
```

Account and group records come from local directory-table enumeration and intentionally filter system-account names. They are not a complete directory, permission, or group-membership audit. Windows SIDs are NULL.

`Environment` describes the environment inherited by this invocation of `wmic`. Demonstrate it with a nonsecret variable:

```sh
WMIC_EXAMPLE='hello from macOS' wmic environment where "Name='WMIC_EXAMPLE'" get Name,VariableValue /value
```

This does not create a persistent system environment variable. Avoid exporting complete environments when you only need one variable; environments can contain application credentials.

## Filters and empty results

The in-memory predicate evaluator supports scalar comparisons, `AND`, `OR`, `NOT`, parentheses, `LIKE`/`NOT LIKE`, and `IS NULL`/`IS NOT NULL`. It is a bounded WQL predicate subset, not a general SQL engine. There is no SQL `JOIN`, `ORDER BY`, or aggregation interface in these examples.

```sh
wmic process where "(Name='Safari' OR Name='Finder') AND ProcessId > 1" get ProcessId,Name
wmic os where "SerialNumber IS NULL" get Caption,SerialNumber /value
wmic os where "Version='wmic_no_such_version'" get Caption /value
```

The last command intentionally selects no instances. It can print `No Instance(s) Available.` to stderr while exiting **0**. Empty results are not automatically failures; distinguish “no matching record” from an invalid property or unsupported class. A nonempty stderr stream alone is not a reliable failure test.

## Full paths and class schemas

Use a class name to query instances without an alias:

```sh
wmic path Win32_OperatingSystem get Caption,Version /value
wmic path '\\.\root\cimv2:Win32_OperatingSystem=@' get Caption /value
wmic path "Win32_Process.Handle=\"$$\"" get Name,ProcessId /value
```

`Win32_OperatingSystem=@` selects the singleton. `Handle` is the string-valued key of `Win32_Process`; a full instance path must include all keys. Local-dot, local IPv4/IPv6, and local hostname prefixes are validated before selection. A qualified path cannot bypass the local-node or namespace restrictions.

Choose the absolute namespace explicitly:

```sh
wmic '/namespace:\\root\cimv2' path Win32_OperatingSystem get Caption
```

Keep the leading two backslashes inside the quoted switch. A relative namespace is appended to the current namespace, which can produce an unintended namespace.

Inspect the class schema separately:

```sh
wmic class Win32_Process
```

`CLASS` returns schema metadata rather than process instances. Inherited Windows qualifiers and help can advertise unsupported operations; they are not the macOS capability list.

## Choose a format

| Need | Example |
| --- | --- |
| A readable table | `wmic os get Caption,Version` |
| Named properties | `wmic os get Caption,Version /value` |
| Classic CSV | `wmic os get Caption,Version /format:csv` |
| Raw CIM XML | `wmic os get Caption,Version /format:rawxml` |
| MOF | `wmic os get Caption,Version /format:mof` |
| HTML table | `wmic os get Caption,Version /format:htable` |
| HTML form | `wmic os get Caption,Version /format:hform` |

Classic CSV has legacy escaping and array behavior. It is not guaranteed to be RFC 4180 CSV, and splitting a row on commas can corrupt fields. Native JSON output is not implemented; the [JSON summary example](../../examples/macos/README.md#json-os-summary) wraps a deliberately small singleton `/value` query.

Quote a wildcard so zsh does not expand it:

```sh
wmic os get '*' /value
```

Large full-property queries contain many intentional NULLs. Select only the fields you need when building an inventory script.

### Custom stylesheet

From the repository root, the supplied local XSLT example renders just the requested OS properties as text:

```sh
wmic os get Caption,Version '/format:"examples/macos/os-summary.xsl"'
```

Keep the double quotes around the stylesheet path for WMIC itself; the outer single quotes stop the shell removing them. An unquoted path containing `/` in a GET format switch can be parsed as another switch and report `Invalid GET switch`.

The [stylesheet](../../examples/macos/os-summary.xsl) uses XSLT 1.0 through the system libxslt. It does not execute scripts or access the network. Custom stylesheets with Microsoft script extensions or a document type declaration are refused; file writes and network access are disabled. Custom serialization can differ from MSXML6, as documented in the [stylesheet reference](../macos.md#formats-and-stylesheets).

## Take a bounded sample

```sh
wmic cpu get LoadPercentage /every:1 /repeat:3
```

Use a finite `/REPEAT` in automation. An unbounded `/EVERY` command keeps running until interrupted. Ctrl+C cancels a foreground command; a cancelled macOS CLI ends by SIGINT.

## Power methods

The power methods request real machine shutdown or restart and need root. They are not dry-run operations. Ordinary requests use `/sbin/shutdown` to coordinate with launchd; explicit force flags use the kernel path. This is an administrative shutdown sequence, with no promise of GUI document prompts or cancellation.

The [reference](../macos.md#methods) documents the method names, accepted flags, return codes, and injected-only acceptance boundary. No power action is needed for any example in this cookbook or the examples directory, and no power action was used to verify them.

Continue with the [scripting guide](scripting.md) for quoting, output files, JSON, exit handling, and session behavior.
