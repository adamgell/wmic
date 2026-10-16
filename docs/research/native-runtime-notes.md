# Native runtime observations

The pinned ARM64 WMIC `10.0.26100.1` has SHA-256 `2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C`. Its en-US resource module has SHA-256 `5E3BC78AF6EB1AEA40C6899C452CA8701D28756BF1EB5F056207FC963CFEA17B`. Reference binaries are kept only in ignored `.reference-bin/` for local inspection.

## Fail-fast

The original imports `IcmpCreateFile`, `IcmpSendEcho`, `Icmp6CreateFile`, `Icmp6SendEcho2`, and `IcmpCloseHandle` from IPHLPAPI. This is an ICMP preflight, not a timeout imposed on native WMI requests.

ARM64 disassembly at image base `0x140000000` establishes the default:

- `0x140029E84` passes the enclosing object plus `0x200` to the settings constructor at `0x1400278C8`.
- `0x140027930` loads `0x138800000000`; `0x14002793C` stores it at settings offset `0xB0`. The upper DWORD is 5000, at enclosing offset `0x2B4`.
- `0x1400343EC` loads that DWORD into `w1` for the ping helper at `0x140034BA0`.
- The helper saves it in `w21` at `0x140034BD0`, moves it into `w7` at `0x140034DA8`, and calls the `IcmpSendEcho` import at `0x140034DC4`. `w7` is the eighth ARM64 argument, the timeout in milliseconds.
- The IPv4 request is 32 bytes, uses TTL 128 and an 8184-byte reply buffer. The payload loops over the first 23 lowercase letters.
- The numeric `/FAILFAST:125` option is accepted by the original; its setter stores a positive parsed integer into the same DWORD at `0x14002E1E8`. Captured `/FAILFAST:ON` and numeric context both display `ON`.

These observations support a default 5000 ms ICMP timeout. They do not establish timeout behavior on every Windows build or network.

## XML and XSL

`tests/fixtures/connection-reference/trace.stderr.bin` identifies `CLSID_FreeThreadedDOMDocument60`, `CLSID_XSLTemplate60`, and `IXSLTemplate`/`IXSLProcessor`: the selected profile uses MSXML 6. The same trace shows `IWbemObjectTextSrc::GetText` with `WMI_OBJECT_TEXT_CIM_DTD_2_0`, and a WMI context named `ExcludeSystemProperties`.

Built-in text output loads `texttable.xsl` from the original WBEM directory. The replacement must provide its own formatting assets and must not require those installed files at runtime. Custom transforms should use the native MSXML 6 engine.

## COM security

The default context is authentication level `PKTPRIVACY` (6), impersonation `IMPERSONATE` (3), privileges enabled. The original trace requests `RPC_C_AUTHN_DEFAULT` when setting each proxy blanket.

Set a blanket on the actual interface pointer. Querying `IUnknown` first can select a different proxy; the first adapter implementation did that and a real `IEnumWbemClassObject::Next` returned `0x80041003`. Borrowing the original pointer fixed the query. Apply credentials and security to both `IWbemServices` and enumerators.

Microsoft documents the credential copy lifetime in [CoSetProxyBlanket](https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-cosetproxyblanket), and the signed Automation storage used for unsigned CIM types in [Numbers (WMI)](https://learn.microsoft.com/en-us/windows/win32/wmisdk/numbers).
