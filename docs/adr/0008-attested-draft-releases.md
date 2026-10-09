# ADR 0008: Release Exact-Version Tags As Attested Drafts Of Checked Builds

- Status: Accepted
- Date: 2026-10-09

## Context

A firmware image is flashed by a person holding a probe, often from a downloaded
file. They need to know which source produced it and that it is the same binary
that passed CI. A green build is not evidence of hardware qualification.

## Decision

- A release tag must equal `v` plus the exact Cargo package version.
- Packaging reuses the embedded job's checked firmware by immutable artifact ID;
  nothing is rebuilt for release.
- The package carries ELF/HEX, the self-test ELF, build inputs,
  `BUILD-INFO.json`, `SHA256SUMS`, and a GitHub artifact attestation signed with
  the workflow's OIDC identity.
- Signing and release publication run in separate jobs with separate
  permissions, and the workflow only ever creates or updates a **draft**.
  A tag whose release is already published fails instead of being rewritten.

## Rationale

Exact tags and artifact reuse remove "which build is this?" ambiguity. Draft
publication keeps the human hardware and release review explicit.

## Consequences

- Verifiers check provenance with `gh attestation verify` against the reviewed
  commit before flashing; see [deployment](../deployment.md#verify-before-flashing).
- Attestations do not provide secure boot, signed device updates, or rollback
  protection; those remain separate roadmap work.
- Changing a published release means tagging a new version.
