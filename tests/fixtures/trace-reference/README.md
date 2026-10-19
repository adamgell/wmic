# TRACE reference captures

Captured on 2026-10-19 from signed Microsoft ARM64 WMIC 10.0.26100.1, SHA256 `2ef794c4940afea56c6d6d3b9bc9e66965bc7333e79c5dcb2def3c0e36f0834c`, on the existing Windows ARM64 reference machine. `capture.json` records the executable identity, profile, full process status and hashes of both streams. `../trace-cases.json` records the inputs. The only declared normalization is the existing COMMAND STARTTIME attribute policy for RAWXML stdout; stderr is compared without normalization.

The Microsoft executables and format resources were absent from their ordinary directories. The pinned original and en-US MUI were staged under ProgramData; the existing preserved Microsoft format data was restored for these captures. No Microsoft executable was placed in a system directory. No original executable, MUI or stylesheet is distributed with this repository.

Portable tests supply measured provider outcomes (including six processor instances on this reference machine) and compare complete diagnostic bytes. The Windows CLI tests use stable queries/errors; they do not assume every machine has six processors or the same OS caption. Interactive fixtures cover a cold traced session, a second query with cached resources, OFF, and enabling TRACE after an untraced query.
