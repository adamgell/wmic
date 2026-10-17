#!/bin/sh
# Read-only queries, UTF-8 reports, and a fresh directory for every invocation.
set -eu
umask 077

if [ "$#" -gt 1 ]; then
    printf 'Usage: %s [output-parent-directory]\n' "$0" >&2
    exit 64
fi
if [ "$(uname -s)" != Darwin ]; then
    printf '%s\n' 'This example requires macOS.' >&2
    exit 1
fi
wmic_bin=${WMIC_BIN:-wmic}
if ! command -v "$wmic_bin" >/dev/null 2>&1; then
    printf 'Cannot find wmic: %s. Set WMIC_BIN to the built executable.\n' "$wmic_bin" >&2
    exit 1
fi
output_parent=${1:-.}
mkdir -p "$output_parent"
report_dir=$(mktemp -d "$output_parent/wmic-inventory.XXXXXX")
printf 'Writing inventory to %s\n' "$report_dir" >&2

query() {
    report_name=$1
    shift
    "$wmic_bin" "$@" > "$report_dir/$report_name.txt"
}

query os os get Caption,Version,BuildNumber,OSArchitecture /value
query cpu cpu get Name,NumberOfCores,NumberOfLogicalProcessors /value
query computer computersystem get Name,Manufacturer,Model,SystemType,TotalPhysicalMemory /value
query logical-disks logicaldisk get DeviceID,VolumeName,FileSystem,Size,FreeSpace /value
query adapters nic get Index,NetConnectionID,MACAddress,NetEnabled /value
query network-config nicconfig get Index,IPAddress,IPSubnet,DefaultIPGateway,DNSServerSearchOrder /value
query processes process get ProcessId,ParentProcessId,Name /value

# Only the final directory path goes to stdout, so a caller can capture it.
printf '%s\n' "$report_dir"
