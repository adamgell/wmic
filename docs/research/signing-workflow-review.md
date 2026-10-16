# Signing workflow review, 2026-10-16

One read-only reviewer assessed `cd7f549..27797f3`, the initial GitHub CI and OIDC signing workflow. No application Rust behavior changed. The reviewer ran all 10 release Python tests at that revision, eight certificate identity cases, and actionlint. The review found one Important issue and no Critical or Minor findings.

## Finding and disposition

The final shell checks treated release/tag lookup failure as absence. A transient failed tag lookup followed by successful draft creation could bind the draft to an existing tag at a different source commit. `gh release create --target` does not move an existing tag.

Fixed by `release.verify_available`: both complete paginated GitHub inventories must be read successfully and parse as valid pages/entries. An exact existing release or tag is rejected. Authentication, transport, process and parse failures abort. This gate runs before building and immediately before draft creation. A behavioral regression failed before implementation, then the full 11-test release suite passed. Cases cover absence, an existing release, an existing tag, malformed inventories/JSON and a failed API process. The read-only gate also passed against the actual repository for `v0.1.0-dev`; actionlint and whitespace validation passed.

The reviewer concluded that fixing this issue makes the change ready for the bounded native release run. No second whole-change review was performed.

## Evidence boundaries retained

The reviewer set aside live Azure configuration, hosted signing/verification, independent downloaded-archive acceptance, historical Defender disposition and runtime compatibility of signed CI builds. These are retained as follows:

- Live configuration was independently read back by the implementing agent: one exact environment federated credential, no key/password credentials, one profile-scoped Signer assignment, and only `main` permitted in the GitHub environment.
- Hosted OIDC authentication, signing, native verification and packaging must succeed in the actual workflow before a signed-release claim.
- Downloaded packages must match their manifest/receipt hashes. Native signature validation on those extracted bytes is separate acceptance evidence.
- The historical Defender detection remains unresolved; signing does not settle it.
- The measured compatibility corpus applies to its documented unsigned baseline. A CI rebuild and signing do not expand that runtime evidence.
