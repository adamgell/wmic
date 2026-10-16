# Signed GitHub releases

The public repository is [adamgell/wmic](https://github.com/adamgell/wmic). Azure Artifact Signing (formerly Trusted Signing) retains the signing key. GitHub obtains a short lived Azure token using OpenID Connect; it stores no signing private key or Azure client secret.

WMIC uses its own Entra application and service principal, with permission to sign through the same certificate profile used by CMTrace Open. CMTrace Open's authentication and release configuration remain independent.

## Identity and configuration

The Azure tenant is Default Directory, `69cd6393-d1ce-4cf2-8f2b-45c39003aa34`. The dedicated application `github-wmic-signing` has client ID `d500d5be-0ade-4e8d-a027-1a36e78be8be`. Its single federated credential is:

| Field | Value |
| --- | --- |
| Issuer | `https://token.actions.githubusercontent.com` |
| Subject | `repo:adamgell@27519/wmic@1422255455:environment:codesigning` |
| Audience | `api://AzureADTokenExchange` |

This repository uses GitHub's immutable subject format, which includes owner ID `27519` and repository ID `1422255455`. The exact prefix was read from `gh api repos/adamgell/wmic/actions/oidc/customization/sub` and confirmed by the first workflow's Azure login diagnostics. A name-only subject does not match this repository's tokens. The original name-only trust was replaced after the first attempt failed closed at Azure authentication. Future setup should read the live `sub_claim_prefix` before creating a federated credential. See [GitHub's immutable subject announcement](https://github.blog/changelog/2026-04-23-immutable-subject-claims-for-github-actions-oidc-tokens/) and [OIDC API reference](https://docs.github.com/en/rest/actions/oidc).

The identity has **Artifact Signing Certificate Profile Signer** at the exact certificate profile resource scope: subscription `7c7ca912-d5ea-4a1f-8cad-f33afb9d99f4`, resource group `CodeSigning`, account `adamgell4`, profile `adamgell-github`. This active PublicTrust profile was confirmed against the certificate history and the signature receipt from CMTrace Open release 1.6.2. The identity does not need Contributor, Owner, certificate profile administration, or access to another application's secrets.

The GitHub `codesigning` environment permits deployments only from the `main` branch. Its environment variables contain these identifiers and configuration:

| Variable | Meaning |
| --- | --- |
| `AZURE_CLIENT_ID` | WMIC's dedicated application's client ID |
| `AZURE_TENANT_ID` | Default Directory tenant ID above |
| `AZURE_SUBSCRIPTION_ID` | Subscription containing the signing account |
| `SIGNING_ENDPOINT` | `https://eus.codesigning.azure.net/` |
| `SIGNING_ACCOUNT_NAME` | `adamgell4` |
| `SIGNING_PROFILE_NAME` | `adamgell-github` |
| `SIGNING_PUBLISHER_NAME` | Exact expected publisher CN and organization, `Adam Gell` |

These are identifiers and configuration, not credentials. Do not add `AZURE_CLIENT_SECRET` or export a PFX. The workflow's Azure credential chain permits only the Azure CLI session established by `azure/login` with OIDC.

## Release flow

1. Commit the intended version in `Cargo.toml` and merge the reviewed changes to `main`. Run ordinary CI first. The release tag must be exactly `v` plus the Cargo version.
2. Manually run **Signed development release** from `main`, with that tag. Other branches, pull requests and repositories cannot pass preflight. Concurrent release runs are serialized.
3. Portable validation runs without Azure permissions. Separate Windows x64 jobs compile locked dependencies for ARM64, x64 and x86 with static CRT linkage. Only the x64 target is executed on the x64 runner. Build metadata binds each executable to its architecture, source SHA, workflow run and attempt. Unsigned executables must fail native signature verification.
4. The signing job downloads the current run's three inputs and verifies the complete inventory, hashes, architecture and provenance before Azure authentication. It signs only their executables using SHA256 and Microsoft's RFC3161 timestamp service. No reference binaries or fixtures are eligible inputs.
5. Windows validates the trusted Authenticode signature, publisher identity, code signing EKU, Microsoft timestamp identity and timestamp EKU. Windows SDK SignTool must also pass `/pa /all /v /tw`. The gate tests rejection of a wrong publisher and a modified signed PE section. Receipts bind the verification result to the signed byte hash and run context.
6. Packaging occurs after verification, preserving the signed bytes. Each ZIP contains exactly `wmic.exe`, `README.md`, `compatibility.md`, `manifest.json` and `SHA256SUMS`. The manifest embeds verification metadata and the receipt hash. Rechecking the complete bundle requires all three ZIPs, their checksum sidecars, three signature receipts, and `release-context.json`.
7. A separate job with repository write permission creates a **draft prerelease**. It has no Azure ID token permission. Existing versions are not overwritten or retagged. Review the draft before publishing it.

The signing job has `contents: read` and `id-token: write`; build jobs have `contents: read`; only the draft job has `contents: write`. Checkout does not persist credentials. Actions are pinned to full commit IDs, and signing dependencies are installed without restoring a cache from another workflow.

## Verification and limits

Use Python 3.12 for `tools/release/release.py` and the release tests. Run `actionlint .github/workflows/*.yml`, the Python release suite, and `pwsh -File tools/release/tests/signature-policy.Tests.ps1` before changing this pipeline. Certificate policy tests use ephemeral test certificates; they do not install trust or prove a real executable's signature. Native Windows verification in the actual release run supplies that evidence. JSON receipts record that verification and provide integrity binding; they are not a substitute for independently checking a downloaded executable's signature.

After extracting a release ZIP on Windows:

```powershell
Get-AuthenticodeSignature .\wmic.exe | Format-List Status,SignatureType,SignerCertificate,TimeStamperCertificate
Get-FileHash .\wmic.exe -Algorithm SHA256
```

Expect `Valid`, `Authenticode`, publisher `Adam Gell`, and a Microsoft timestamp. Compare the executable hash to the package manifest and receipt, and the archive hash to its sidecar. Artifact Signing rotates short lived leaf certificates, so a fixed leaf thumbprint is not used as a permanent publisher identity.

The compatibility report describes a separately measured local unsigned baseline. Signing proves origin and integrity; it does not extend the measured OS/provider coverage or resolve the historical Defender detection. Keep those limits in the release notes.

## First verified prerelease

The [signed workflow, attempt 2](https://github.com/adamgell/wmic/actions/runs/38917773797/attempts/2) succeeded for source `8117295f3b8e562cb97caa847604c9e4b39d5c45` and created the draft for [prerelease `v0.1.0-dev`](https://github.com/adamgell/wmic/releases/tag/v0.1.0-dev). [CI on that source](https://github.com/adamgell/wmic/actions/runs/38917717172) passed all four jobs. The first release attempt stopped at the name-only OIDC subject mismatch described above; the second authenticated successfully using the corrected immutable subject.

All three executables passed Windows Authenticode and SDK SignTool verification before packaging. Each architecture's unsigned, wrong-publisher and tampered-section negative checks rejected their inputs. The draft has exactly ten assets: three ZIPs, three ZIP checksum sidecars, three native signature receipts, and release context. All downloaded asset digests were compared to GitHub's asset metadata; the complete downloaded bundle passed provenance, inventory, PE and hash checks.

Independent read-only Authenticode checks on the selected Parallels guest then verified the actual executable bytes extracted from those downloaded draft ZIPs. All three returned `Valid` and `Authenticode` with the expected publisher and Microsoft timestamp identity/EKU. This check did not execute CLI operations. The [native evidence](research/evidence/2026-10-16-signing/downloaded-release-verification.json) records these signed executable hashes:

| Architecture | Signed executable SHA256 |
| --- | --- |
| ARM64 | `cf8421a3dcdaf66a73f3a57a730d03f2a4704fa86b9d2a7a24c93aff971ce1c5` |
| x64 | `94274a0b2f0462e3374c864fb4a52ce47fdc0df9daf196a4fdc0c8589b7d35ac` |
| x86 | `af470d7643d49721a0dc07d74115fffc9b9518b666b1e314a6fe4188ee4d335d` |

The user authorized publication on 2026-10-16, and the release is now a public prerelease. An unauthenticated GitHub API read confirmed publication, the tag resolves to the verified source commit, and all ten asset digests and the release notes match the verified draft.

References: [Microsoft OIDC instructions](https://github.com/Azure/artifact-signing-action/blob/main/docs/OIDC.md), [Azure GitHub OIDC integration](https://learn.microsoft.com/en-us/azure/developer/github/connect-from-azure-openid-connect), [Artifact Signing action](https://github.com/Azure/artifact-signing-action).
