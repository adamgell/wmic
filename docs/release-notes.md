Development release of the native Rust WMIC compatibility CLI for Windows ARM64, x64 and x86.

The release workflow signs each `wmic.exe` through Azure Artifact Signing using the existing Adam Gell certificate profile. Native Windows Authenticode and SDK SignTool verification must succeed before packaging. Each archive preserves the signed executable bytes; separate signature receipts, SHA256 sidecars and release context identify the source commit, architecture, workflow run and exact hashes.

Full classic WMIC compatibility is not claimed. The [compatibility report](https://github.com/adamgell/wmic/blob/main/docs/compatibility.md) describes 817/820 exact corpus matches per architecture plus controlled comparisons for the separately measured local unsigned baseline. GitHub builds use a different build environment and signing changes the binary hashes. Native AMD64/x86 hardware, additional Windows versions/locales, ordinary-user profiles, cross-machine authentication and certain provider operations remain unverified.

An earlier unsigned candidate was quarantined by Defender as `Trojan:Win32/Bearfoos.A!ml`. The historical detection remains unresolved. Signing and later successful tests do not establish a false positive or clear that detection. Protection settings were left enabled and unchanged.

Extract the ZIP for your architecture, verify the signature and hashes, and run `wmic.exe /?`. The executable supports process-local PATH precedence; this package does not replace a Windows system file or install a service. Review the included README and compatibility report before deployment.
