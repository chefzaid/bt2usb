# ADR 0008: Release Exact-Version Tags As Attested Drafts Of Checked Builds

- Status: Accepted
- Date: 2026-09-28

## Context

A bt2usb image is flashed by a person holding a debug probe, often from a
downloaded file. Before flashing they need to know which source produced it,
that it is the same binary that passed CI, and whether anyone has checked it on
hardware. A green build answers only the second question, and only if the
release really contains the bytes CI checked.

The first release job, added on 2026-06-23 (`dc11b4a`), answered none of them
reliably:

- It ran on any `v*` tag without comparing the tag with the Cargo package
  version.
- It rebuilt the firmware inside the release job, so the published bytes were
  not the bytes the `embedded` job had linted and built.
- It installed the latest stable Rust (`dtolnay/rust-toolchain@stable`) and
  referenced actions by movable tags such as `@v2` and `@v4`, so two runs of the
  same commit could build differently.
- The job that built the firmware also held `contents: write` and published a
  final, non-draft release with ELF and HEX files only: no checksums, build
  inputs, or provenance.

The hardening commit of 2026-09-28 (`2479c79`) replaced it with the design
below.

## Decision

- **Exact version tags.** A release tag must be `v` followed by the exact
  `package.version` from `Cargo.toml`, including any prerelease and build
  metadata. A prerelease version produces a prerelease draft.
- **No rebuild.** The `embedded` job builds and lints the firmware once, stages
  it with its build inputs and metadata, and uploads it. Packaging downloads
  that upload by its immutable artifact ID, verifies it, and never compiles.
- **Verified identity.** Staging refuses a modified tracked source tree or a
  checkout that differs from the workflow's commit, and records the source
  commit, ref, repository, run ID and attempt, compiler and Cargo versions,
  target, profile, features, log level, and the SHA-256 of every input and
  firmware file in `BUILD-INFO.json`. Packaging checks the version, source
  commit, tag ref, repository, run ID, target, profile, features, and log level
  against expected values; the recorded compiler against the channel in
  `rust-toolchain.toml`; the input digests against the checked-out
  `Cargo.toml`, `Cargo.lock`, and `rust-toolchain.toml`; and the firmware
  digests against the staged files. The run attempt and Cargo version are
  recorded but not compared.
- **Package contents.** `bt2usb-<tag>.elf`, `bt2usb-<tag>.hex`,
  `bt2usb-selftest-<tag>.elf`, `Cargo.toml`, `Cargo.lock`,
  `rust-toolchain.toml`, `BUILD-INFO.json`, `SHA256SUMS` over those files, and
  `provenance.sigstore.json`. The SoftDevice is not included.
- **Attestation.** A GitHub artifact attestation, signed with the workflow's
  OIDC identity by the SHA-pinned `actions/attest` action, covers every package
  file, including `SHA256SUMS`.
- **Separated permissions.** Signing and publication run in separate jobs.
  `release-package` has `contents: read`, `id-token: write`, and
  `attestations: write` and cannot publish. `release` has only
  `contents: write`, downloads the attested package by ID, and runs no
  checked-out script.
- **Drafts only.** The workflow only creates or updates a **draft** release. A
  person publishes it after the hardware and release review in
  [deployment](../deployment.md#release-gates). A rerun may refresh an
  unpublished draft, but if the tag's release is already published, or the
  release API cannot be queried, the job fails before uploading anything.
- **Filled release notes.** Added on 2026-10-10: after attestation,
  `release-package` fills [.github/release-notes.md](../../.github/release-notes.md)
  with `release.py notes` from the verified package and the tagged source
  (SoftDevice prerequisite, compiled limits, storage version, checksums) and
  uploads the text as its own artifact. `release` uses it as the draft's
  description, followed by GitHub's generated notes. The notes are editable
  text for reviewers, so they are not a package file and are not attested
  ([release notes](../deployment.md#release-notes)).

## Alternatives Considered

- **Rebuild in the release job.** This was the 2026-06-23 design. It is simple,
  but the shipped bytes are not the checked bytes, the release job needs a
  toolchain and write permission together, and nothing proves the two builds
  match.
- **Publish automatically.** Also the 2026-06-23 design. It removes a manual
  step, but it turns a green build into a published release without any
  hardware review, which the project's
  [release gates](../deployment.md#release-gates) require.
- **Checksums only.** A `SHA256SUMS` file downloaded next to the binary proves
  only that the two files agree, not who produced them. The attestation
  authenticates the checksum manifest itself.
- **Sign with a maintainer-held key.** A long-lived GPG or cosign key pair
  would need custody, rotation, and a published public key, and it would prove
  who held the key rather than which workflow and commit built the files. The
  OIDC-based attestation binds the artifacts to the repository, workflow, ref,
  and commit without a stored secret.
- **One job that builds, signs, and publishes.** Fewer jobs, but every step
  would run with every permission. Splitting them means the code that can sign
  cannot publish, and the code that can publish cannot sign or build.
- **Let reruns replace published assets.** Reruns are useful while a draft is
  under review, but a published release must never change under people who have
  already verified it. Changing a published release means tagging a new
  version.
- **Rely on reproducible builds instead.** Independent rebuilds that match
  byte for byte would be stronger evidence, but they have not been
  demonstrated; that is an open item, not a substitute for provenance.

## Rationale

Exact tags and artifact reuse remove "which build is this?" ambiguity: the tag
names the version, the version names the commit, and the commit names the bytes
that passed lint, tests, simulation, and the dependency audit. Packaging that
refuses any mismatch turns a configuration mistake into a failed job instead of
a misleading release.

Provenance lets a person verify origin from a clean machine with
`gh attestation verify`, constrained to the repository, the signer workflow,
the tag, and the reviewed commit. Draft publication keeps the human hardware
and release review explicit: a draft is a candidate, never evidence that the
review passed.

## Consequences

Positive:

- Every package identifies its source, toolchain, inputs, and firmware hashes,
  and that claim is signed by the workflow that produced it.
- The released firmware is byte for byte the firmware that CI checked.
- A compromised or buggy publishing step cannot sign, and the signing step
  cannot publish.
- Each draft starts with the facts a deployment needs (the exact SoftDevice,
  compiled limits, storage version, checksums) taken from the code that was
  built, with a `REVIEW:` line for each fact only a person can supply.

Negative:

- Releasing needs a version bump in `Cargo.toml` before tagging, and a person
  to review and publish each draft.
- The release must come from the same workflow run as the checked build;
  artifacts from an earlier run are not reused.
- Release builds log at `info` (`DEFMT_LOG: info` in CI, and packaging rejects
  any other level); bring-up uses local builds at `debug`.
- Attestations prove origin and integrity only. They do not establish hardware
  compatibility, the absence of vulnerabilities, reproducibility, or anything
  the device enforces: there is no secure boot, signed update, or rollback
  protection.
- A rerun replaces the draft's description with freshly filled notes, so
  reviewer edits made before a rerun are lost.
- The release path is configured but unproven: no tag has been pushed, so
  packaging, attestation, and publication have never run (see
  [Verification Status](#verification-status)). Local tests cannot issue OIDC
  credentials or call the release API.

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Hosted provenance and release recovery acceptance": run a tag build, verify
  the downloaded package against the approved commit from a clean machine, and
  document rollback and recovery.
- "Reproducible firmware evidence": compare builds from two clean
  environments and enforce size budgets.
- "Supply-chain and tooling maintenance": add license checks and an SBOM to the
  package.
- "Signed USB/BLE DFU": device-side signature enforcement is separate work.

## Implementation

| Concern | Where |
| --- | --- |
| Tag rule | `validate_tag` and `package_version` in [release.py](../../scripts/release.py), with a SemVer pattern that rejects leading zeros in numeric prerelease identifiers; run as `release.py validate-tag` in the host-test job on tags and again in `release-package` |
| Staging | `stage_build` (`release.py stage`) in the `embedded` job, writing to `$RUNNER_TEMP/release-input` and refusing an existing directory; uploaded as `bt2usb-checked-firmware-<run_attempt>` with the job output `firmware-artifact-id` |
| Packaging | `verify_staged` and `package_release` (`release.py package --expected-commit --expected-repository --expected-run-id`) in `release-package`, after `actions/download-artifact` with `artifact-ids` and `digest-mismatch: error` |
| Attestation | `actions/attest` with `subject-path: dist/*` and `create-storage-record: false`; the bundle is copied to `dist/provenance.sigstore.json` and uploaded as `bt2usb-attested-release-<run_attempt>` |
| Publication | The `release` job's "Refuse to modify a published release" step (`gh api --paginate .../releases`), then `softprops/action-gh-release` with `draft: true`, the prerelease flag, `target_commitish` set to the tagged commit, `fail_on_unmatched_files: true`, `body_path` pointing at the filled release notes, and generated notes appended after them |
| Ordering and permissions | `release-package` needs `lint-and-test`, `coverage`, `audit`, `embedded`, and `simulation`; both release jobs run only for tag pushes; the workflow default is `contents: read`; tag runs are never cancelled by a newer run (`cancel-in-progress` is false for tags) |
| Action pins | Every action is pinned to a commit SHA with a comment naming its exact upstream release, in [ci.yml](../../.github/workflows/ci.yml); the release jobs use `actions/download-artifact` v8.0.1, `actions/attest` v4.2.2, `actions/upload-artifact` v7.0.1, and `softprops/action-gh-release` v3.0.3, all on Node 24, on the pinned `ubuntu-24.04` image |
| Helper tests | [release_test.py](../../scripts/release_test.py), run on Linux and Windows in CI; `grep -c 'def test_'` counts 17 tests covering exact and prerelease tags, tampering, metadata identity, changed lockfiles, rehashed artifacts, checksum entries, overwrites, staging identity, dirty checkouts, and the filled release notes |

Error messages are specific, for example
`release tag must be exactly v{version}; received {tag!r}`,
`refusing to stage a build from a modified tracked source tree`,
`build input differs from release source: {name}`, and
`build compiler does not match the pinned Rust toolchain`. A rerun against a
published release fails with
`$RELEASE_TAG is already published ($published). Tag a new version instead of re-running.`

### Verification Status

- **Implemented:** everything in the table above.
- **Software-verified:** the 17 release-helper tests and actionlint pass in
  the CI host-test job, and the `embedded` job stages and uploads the checked
  firmware on every run. Those check jobs passed on GitHub-hosted runners in
  push runs 36441995385 (`8a04b25`, 2026-09-28) and 37932436721 (`7fc99d6`,
  2026-10-09) and scheduled run 37338711407 (2026-10-05). The tag-only
  `release-package` and `release` jobs have never run, and no `v*` tag or
  release exists, so tag validation in CI, packaging against a real run,
  attestation, the published-release guard, and draft creation are
  unverified.
- **Hardware-verified:** not applicable to the pipeline. A release still
  needs the hardware review in
  [deployment](../deployment.md#release-gates) before a person publishes it.

## Related

- [Deployment: artifact flow](../deployment.md#artifact-flow),
  [verify before flashing](../deployment.md#verify-before-flashing), and
  [release gates](../deployment.md#release-gates)
- [Security: firmware integrity and updates](../security.md#firmware-integrity-and-updates)
- [Testing: continuous integration](../testing.md#continuous-integration)
- [ADR 0004: Verify in layers](0004-layered-verification.md)
- [ADR 0007: Vendored nrf-softdevice patch](0007-vendored-softdevice-patch.md)
- [ADR 0013: Pinned toolchain and mask tasks](0013-pinned-toolchain-and-mask-tasks.md)
