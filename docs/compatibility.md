# Compatibility status

Development build, 2026-10-15. Full classic compatibility is not claimed. Final platform and corpus results are being assembled from the selected reference guest; the detailed evidence remains under `docs/research/evidence/2026-10-15` in the source checkout.

The native ARM64 executable has been exercised against Microsoft WMIC 10.0.26100.1 on Windows 11 Pro 25H2 ARM64, build 26200.8037, en-US, as SYSTEM. x64 and x86 packages are also built; runtime results are recorded separately from compilation.

The stable query, help, schema, method, formatting, routing, and interactive cases use strict stdout, stderr, and full-DWORD comparisons. Only explicitly selected command/record timestamps and precisely identified owned process IDs are excluded. Raw captures are retained.

Known differences: implementation-specific TRACE output; credentials redacted from XML/record metadata; two volatile OS default-output probes remain raw mismatches. Clipboard live validation, authenticated additional endpoints, IPv6, other Windows/language profiles, and class-repository writes remain unverified. Destructive system methods were not exercised.

An earlier unsigned candidate SHA256 `4a349fa5611eb48daf6018846ccbfaef9f7495b17648908b083b24da63ed766c` was quarantined as `Trojan:Win32/Bearfoos.A!ml` (ThreatID 2147731250, successful protection action). No protection setting was changed. Subsequent corpus execution does not resolve that detection.
