# Reference evidence, 2026-10-15

These files are research captures from the user-selected Windows Parallels
guest. They describe original Windows metadata, not replacement implementation
or compatibility test results.

## Provenance

- Guest: `Windows 11 Home Domain`, UUID
  `{ec3bc428-7a31-4f17-8424-7ce78923f7f3}`.
- Computer name: `DESKTOP-N4H9E7R`.
- Windows 11 Pro ARM64, build `10.0.26200.8037`.
- Transport: `prlctl exec`, through installed Parallels Tools.
- Execution identity: `NT AUTHORITY\SYSTEM`.
- Repository: `root\cli`.
- Capture phase: WMIC optional component installation in progress.

The MOF exports use `System.Management.ManagementBaseObject.GetText` with
`System.Management.TextFormat.Mof`. The guest serialized the text to UTF-8
and base64 before sending it over the transport. The host decoded that
base64 to preserve the serialized bytes independently of terminal encoding.

## Files

| File | Content | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| `aliases-pre-install.mof` | 81 full alias definitions, including embedded metadata | 1173509 | `225355073d5ea037e5bce16548e6f02a487efcadec41fceaa34953089eb41af3` |
| `alias-schema-pre-install.mof` | 10 `MSFT_Cli*` class definitions | 1278 | `b59a4c034357d52ece55e590de876dd2cb53a9b1e99e8740f97f5fee16285fff` |
| `aliases-pre-install.json` | Alias names, target queries, class list, and capture phase | 8496 | `f1f1c8e4eb51e298118d228021e9b88aa80a55336f34e5286a5f9575d1a7bbd1` |

The `pre-install` filename denotes the capture before successful installation
was verified; it does not imply that the servicing operation had not started.
The original WMIC executable was absent at the initial environment check.

The evidence is local research material. No remote repository or publication
destination has been configured for this project.
