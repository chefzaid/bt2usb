# Deployment Guide

This guide covers how a bt2usb build becomes firmware on a board: versioning,
the tag-driven release pipeline, provenance verification, SoftDevice
installation, flashing, rollback, and the gates a deployment release must pass.
The release workflow prepares a draft for review
([ADR 0008](adr/0008-attested-draft-releases.md)). It does not establish
hardware qualification or provide secure boot on the device. Diagnosing a unit
after it is flashed is covered by the [operations runbook](operations.md).

Deployment here means programming a unit through a debug probe. The firmware
has no application bootloader, over-the-air update, or USB firmware-update
path, so every install, update, and rollback is a probe operation (see
[security](security.md#firmware-integrity-and-updates)). The running firmware
does not report its version over USB or RTT; a unit is identified by the
artifact hash recorded when it was flashed.

The five check jobs (host tests on Linux and Windows, audit, embedded build,
and Renode) have passed on GitHub: push run `36441995385` (commit `8a04b25`,
2026-09-28), scheduled run `37338711407` (2026-10-05), and push run
`37932436721` (commit `7fc99d6`, 2026-10-09); run `36441384244` (commit
`2479c79`) failed. The Host coverage job joined them on 2026-10-10
([coverage in CI](code-quality.md#coverage-in-ci)). No `v*` tag or release exists yet, so the tag-only
`release-package` and `release` jobs described below have never run. Their
configuration is checked by `actionlint` and by the release helper's tests;
the first hosted tag run is the open item "Hosted provenance and release
recovery acceptance" in
[TODO.md](../TODO.md#release-provenance-and-supply-chain).

## Delivery Flow

```mermaid
flowchart LR
    tag[Tag vX.Y.Z = Cargo version] --> checks[CI: host, audit, embedded, Renode]
    checks --> package[Verify and attest checked firmware]
    package --> draft[Draft GitHub release]
    draft --> verify[Verify provenance and checksums]
    verify --> flash[Flash with probe-rs]
    flash --> accept[Hardware acceptance record]
    accept --> publish[Publish after release-gate review]
```

Every push and pull request runs the same checks without packaging. Only tags
produce a draft, and only a person publishes it.

| Event | Jobs in [ci.yml](../.github/workflows/ci.yml) | Result |
| --- | --- | --- |
| Push to `main` or `master`, pull request | Host tests (Linux, Windows), dependency audit, embedded build and Clippy, Renode simulation | Checks only; the embedded job also uploads a staged `bt2usb-checked-firmware-<attempt>` workflow artifact, which is not attested or released |
| Weekly schedule (Mondays, 07:23 UTC) and manual dispatch | Same as a push | Re-runs every check, including `cargo audit` against the current advisory database |
| Push of a tag matching `v*` | All of the above, then `release-package` and `release` | An attested draft release for that tag |

Runs for the same ref cancel an older in-progress run, except for tags: a tag
run always completes. The workflow-level environment sets `DEFMT_LOG: info`
for every job, so CI firmware is built at the info log level.

## Version Lifecycle

The firmware version has one source: `package.version` in
[Cargo.toml](../Cargo.toml), currently `0.1.0`. `Cargo.lock` repeats it in the
`bt2usb` package entry, and the release build records it in `BUILD-INFO.json`.
Nothing else carries it: the USB descriptors, the OLED, and the logs do not
show a firmware version, and the pairing-store format has its own version byte
that changes only with the [storage schema](data-model.md#schema-change-rules).

### What The Tag Check Enforces

[scripts/release.py](../scripts/release.py) `validate-tag` reads `Cargo.toml`
and enforces, in order:

1. The package is named `bt2usb` and has an explicit string version.
2. The version is valid Semantic Versioning: `MAJOR.MINOR.PATCH`, an optional
   dot-separated prerelease (numeric identifiers without leading zeros), and
   optional `+` build metadata.
3. The tag is exactly `v` followed by that version, byte for byte.

It prints `Validated release tag <tag>` on success, or
`Release check failed: <reason>` and exits with status 1. In CI it also writes
`version=` and `prerelease=` outputs; a version with a prerelease part produces
a prerelease draft. On a tag, the check runs in both host-test jobs (step
"Require tag to match Cargo package version") and again in `release-package`
before any firmware is touched.

| `Cargo.toml` version | Accepted tag | Draft type |
| --- | --- | --- |
| `0.2.0` | `v0.2.0` | Release |
| `0.2.0-rc.1` | `v0.2.0-rc.1` | Prerelease |
| `1.2.3-0+build.42` | `v1.2.3-0+build.42` | Prerelease |
| `1.2.3+build.42` | `v1.2.3+build.42` | Release (build metadata is not a prerelease) |

Rejected, per [release_test.py](../scripts/release_test.py): a tag without
the `v`, a tag for a different version (`v0.1.1` for `0.1.0`), a prerelease
tag for a stable version, versions such as `01.2.3`, `1.2`, or `1.2.3-01`, and
tags with trailing newlines or path characters.

The check does not enforce that the tag is on `main`, that the version
increased, that the tag is annotated or signed, or that the draft's
[release notes](#release-notes) were reviewed. Those are review steps.

Check a proposed tag without creating or pushing it, and run the helper's
regression tests:

```sh
python scripts/release.py validate-tag --tag v0.1.0
python -B -m unittest discover -s scripts -p release_test.py -v
```

The helper requires Python 3.11 or newer (it uses `tomllib`) and only the
standard library. It never creates a tag, uploads an artifact, or publishes a
release.

### Cut A Prerelease Or Release

1. Pick the version. Use a prerelease such as `0.2.0-rc.1` for a build that
   still needs hardware acceptance; the final release is a separate tag.
2. Change `package.version` in `Cargo.toml`. Every build uses `--locked`, so
   refresh the lockfile entry as well, for example with
   `cargo update --workspace`, and check that the `Cargo.lock` diff touches only
   the `bt2usb` entry. Merge the change through normal review with CI green.
3. Check the tag locally against the merged commit:

   ```sh
   python scripts/release.py validate-tag --tag v0.2.0-rc.1
   ```

4. Tag the reviewed commit and push only that tag:

   ```sh
   git tag v0.2.0-rc.1 <reviewed-commit>
   git push origin v0.2.0-rc.1
   ```

5. Wait for the workflow to create the draft. If a job fails before the draft
   exists, fix the cause and re-run; see
   [re-running a tag workflow](#re-running-a-tag-workflow).
6. [Verify the draft's assets](#verify-before-flashing), flash a unit with
   them, complete the [first-flash checklist](first-flash.md), and attach the
   evidence to the draft.
7. Complete the [release gates](#release-gates), replace every `REVIEW:` line
   in the draft's [release notes](#release-notes), and check the change list
   GitHub generated below them. Publish a prerelease only as a prerelease.

A published tag is final. The `release` job refuses to change a published
release, so a correction needs a new version. If a tag was pushed by mistake
and its draft was never published, delete the draft and the tag before tagging
again; never move a tag whose release is published.

### Release Notes

Each draft's description starts from
[.github/release-notes.md](../.github/release-notes.md). After packaging, the
`release-package` job runs `release.py notes`, which fills the template from
the verified package and the tagged source and fails the job if any of them
disagree; the `release` job then creates the draft with that text as its body,
and GitHub appends its generated list of merged changes. Run the command
locally against a package directory to preview a draft's text:

```sh
python scripts/release.py notes --package dist --tag v0.2.0-rc.1 --output RELEASE-NOTES.md
```

| Section | Filled from | Left to the reviewer |
| --- | --- | --- |
| Heading | Tag, prerelease flag, source commit, repository, and workflow run ID from `BUILD-INFO.json` | |
| Supported Versions | | Which releases receive fixes, per [SECURITY.md](../SECURITY.md), and the hardware acceptance record for this build |
| SoftDevice Prerequisite | The SoftDevice name and version in the header of [memory_sd.x](../memory_sd.x), and its `FLASH` and `RAM` origins; the command fails unless the `softdevice` recipe in `maskfile.md` installs the HEX of that same version | |
| Compatibility Limits | `BLE_MAX_CONNECTIONS`, `MAX_PAIRED_DEVICES`, `USB_VID`, and `USB_PID` in [config.rs](../src/config.rs) | The peripherals, hosts, hubs, and KVMs tested, and limits found in testing |
| Pairing Storage And Migrations | Storage pages from `config.rs`; frame magic and version from [framing.rs](../src/storage/framing.rs) | Whether this release migrates the store, and whether an older release can read the result |
| Rollback Constraints | Fixed text and the storage version | The oldest release this one can roll back to without a factory reset |
| Checksums | `SHA256SUMS`, after checking every entry against its file | |
| Build | `rustc` and `defmt_log` from `BUILD-INFO.json` | |

The template's links point at the guides as of the release tag. The notes are
not a release file: they travel in their own workflow artifact
(`bt2usb-release-notes-<run_attempt>`), so they are neither attested nor in
`SHA256SUMS`, and a reviewer may edit them in the draft. The release helper
tests check that every field is filled, that the template's guide links reach
existing headings, and that a mismatched package or SoftDevice fails.

## Version And Build Policy

The tag rule and the helper that checks it are under
[Version Lifecycle](#version-lifecycle). Release firmware is built with these
fixed inputs, and packaging rejects a build that differs:

| Input | Value | Enforced by |
| --- | --- | --- |
| Toolchain | Rust `1.95.0` from [rust-toolchain.toml](../rust-toolchain.toml) | `BUILD-INFO.json` `rustc` must start with `rustc 1.95.0 ` |
| Dependencies | [Cargo.lock](../Cargo.lock), `--locked` | Lockfile hash must match the tagged source |
| Target and features | `thumbv7em-none-eabihf`, `embedded` | The `embedded` job's build command; `stage` writes both as fixed values, so the packaging comparison checks the metadata, not how the files were actually built |
| Profile | `release`: `opt-level = "s"`, fat LTO, one codegen unit, `debug = 2` | The `--release` build command; `stage` reads only `target/thumbv7em-none-eabihf/release` and records the fixed string `release` |
| Log level | `DEFMT_LOG=info` | Packaging requires `info` |
| Source identity | The tagged commit, which `build.rs` embeds and the boot line reports ([boot sequence](operations.md#boot-sequence)) | `stage` refuses an application or self-test ELF that does not contain the checked-out commit, or contains it followed by `-dirty` |

`debug = 2` keeps DWARF debug information in the ELF for probe-rs; it does not
add code to the flashed image.

## Artifact Flow

1. The `embedded` job builds and lints the ARM firmware. It converts that ELF to
   Intel HEX with the toolchain's `llvm-objcopy` and stages the application,
   self-test, build inputs, and metadata. Staging rejects changes to tracked
   source, a checkout that differs from the workflow's source commit, and an
   image that does not report that commit as clean.
2. After host checks, host coverage, audit, embedded checks, and Renode succeed, `release-package`
   downloads the **immutable artifact ID** emitted by that build job. Artifact
   download digest mismatches fail the job. No firmware is rebuilt in this job.
3. The packaging helper checks every staged checksum, exact tag/version, source
   commit/ref, repository, workflow run ID, target/profile/features, log level,
   compiler version, and checked-out build-input hashes. It preserves firmware
   bytes, adds versioned filenames, and creates the release checksum manifest.
4. The SHA-pinned official `actions/attest` action signs provenance for all
   package files, including `SHA256SUMS`, using GitHub's OIDC identity. The bundle
   is attached as `provenance.sigstore.json` after signing.
5. The packaging job then fills the [release notes](#release-notes) from the
   attested package and uploads them as a separate artifact.
6. A separate `release` job downloads the attested package and the notes by
   their immutable IDs and prepares the draft, with the notes as its
   description. This job has `contents: write` but no signing
   permissions. The packaging job has `contents: read`, `id-token: write`, and
   `attestations: write`; it cannot publish a release. Both run only on tag pushes.

The staged firmware and the Renode results live in the runner's temporary
directory, not in the Rust-cached `target/`, so a restored cache cannot supply
stale files. Staging also refuses an output directory that already exists.

A release for tag `vX.Y.Z` contains:

| File | Content | Flashed |
| --- | --- | --- |
| `bt2usb-vX.Y.Z.elf` | Bridge firmware ELF with debug information and the `defmt` string table | Yes, with `probe-rs run`; also needed to decode RTT logs |
| `bt2usb-vX.Y.Z.hex` | Intel HEX converted from that same ELF | Yes, with `probe-rs download` or another programmer |
| `bt2usb-selftest-vX.Y.Z.elf` | Board self-test image | Only for bring-up; it replaces the bridge |
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | Build inputs from the tagged source | No |
| `BUILD-INFO.json` | Build identity and hashes | No |
| `SHA256SUMS` | SHA-256 of every file above | No |
| `provenance.sigstore.json` | Sigstore attestation bundle | No |

SoftDevice S140 is not part of the package; see
[SoftDevice Installation](#softdevice-installation).

### Re-running a tag workflow

Artifact names include the run attempt, so a rerun never collides with an
earlier attempt's uploads. A rerun is intended to refresh the tag's existing
**draft** with the rerun's attested package, through the pinned
`softprops/action-gh-release` action; this has not yet been exercised by a
hosted run. A rerun also replaces the draft's description with freshly
filled notes, discarding edits made in the draft, so edit the notes after the
last rerun. The `release` job fails before any upload if a **published**
release already exists for the tag, and it also fails if the release API cannot
be queried. To change a published release, tag a new version; do not edit or
re-run the old tag.

`BUILD-INFO.json` records the original build's commit, ref, repository, workflow
run/attempt, compiler/Cargo versions, target, profile, features, log level, input
hashes, and firmware hashes. `Cargo.toml`, `Cargo.lock`, and `rust-toolchain.toml`
are included. Source and vendored patches are identified by the same source
commit; they are not replaced by a later checkout or recompilation.

| Field | Meaning |
| --- | --- |
| `schema_version`, `package`, `version` | `1`, `bt2usb`, and the Cargo version |
| `source_commit`, `source_ref`, `repository` | The 40-character commit, `refs/tags/<tag>`, and `owner/repo` |
| `workflow_run_id`, `workflow_run_attempt` | The run that built the firmware |
| `target`, `profile`, `features`, `defmt_log` | `thumbv7em-none-eabihf`, `release`, `["embedded"]`, `info` |
| `rustc`, `cargo` | Compiler and Cargo version strings |
| `input_sha256` | SHA-256 of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` |
| `artifact_sha256` | SHA-256 of `bt2usb.elf`, `bt2usb-selftest.elf`, `bt2usb.hex`, under their staged (unversioned) names |

The release also contains the application ELF/HEX and self-test ELF. SoftDevice
is obtained separately. `SHA256SUMS` covers package payloads, not itself or the
attestation bundle; the manifest itself is an attested subject, and the bundle's
signature is checked by the verifier.

## Verify Before Flashing

Use a GitHub CLI version supporting `gh attestation verify`. Obtain the approved
release tag and full commit SHA through your review process; a checksum file
downloaded alongside a binary is not, by itself, proof of origin. The commands
below are for Bash/WSL and use this repository's identity. A fork must deliberately
use its own approved repository and workflow identity.

```sh
release_tag='v0.1.0'
approved_commit='REPLACE_WITH_REVIEWED_40_CHARACTER_COMMIT'
gh release download "$release_tag" --repo chefzaid/bt2usb --dir "downloads/$release_tag"
cd "downloads/$release_tag"

# Authenticate the checksum manifest against the expected source and workflow.
gh attestation verify SHA256SUMS \
  --repo chefzaid/bt2usb \
  --signer-workflow chefzaid/bt2usb/.github/workflows/ci.yml \
  --source-ref "refs/tags/$release_tag" \
  --source-digest "$approved_commit" \
  --deny-self-hosted-runners

# Validate every payload against that authenticated manifest.
sha256sum --strict --check SHA256SUMS

# Independently verify the firmware that will be flashed.
gh attestation verify "bt2usb-$release_tag.hex" \
  --repo chefzaid/bt2usb \
  --signer-workflow chefzaid/bt2usb/.github/workflows/ci.yml \
  --source-ref "refs/tags/$release_tag" \
  --source-digest "$approved_commit" \
  --deny-self-hosted-runners
```

All checks must succeed. If you will flash the ELF instead of the HEX, run the
last command on `bt2usb-$release_tag.elf`. Compare `BUILD-INFO.json` with the
approved source and hardware validation record, for example:

```sh
jq '{version, source_commit, source_ref, repository, workflow_run_id, defmt_log, rustc}' BUILD-INFO.json
jq -r '.artifact_sha256["bt2usb.hex"]' BUILD-INFO.json
grep " bt2usb-$release_tag.hex\$" SHA256SUMS
```

The two hashes must be equal: packaging renames files but never changes their
bytes. Use `--bundle provenance.sigstore.json` on the same
verification command to read the supplied attestation bundle instead of fetching
it from GitHub. Fully offline verification also requires a trusted root prepared
in advance; follow GitHub's
[offline verification guide](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/verify-attestations-offline).

Before publication the release is a draft, which GitHub shows only to accounts
with write access to the repository. Reviewers verifying a draft need that
access.

These flags constrain the repository, signer workflow, source ref, and commit;
the CLI checks the artifact digest and attestation signature. See the official
[verification reference](https://cli.github.com/manual/gh_attestation_verify) and
[artifact attestation guide](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations).

## SoftDevice Installation

The application links at `0x00027000` and expects Nordic SoftDevice S140
v7.3.0 below it. [memory_sd.x](../memory_sd.x) reserves flash
`0x00000000–0x00027000` and RAM `0x20000000–0x20006000` for it (see the
[memory layout](hardware.md#memory-layout)). Install it once per board, again
after any full-chip erase, and never mix in a different SoftDevice version or
variant: the bindings and memory map are written for S140 v7.3.0.

Run `mask softdevice`. The [`softdevice` recipe](../maskfile.md#softdevice)
downloads Nordic's `s140_nrf52_7.3.0.zip` with `curl` only when
`s140_nrf52_7.3.0_softdevice.hex` is missing from the repository root,
extracts that HEX, and then always flashes it with `probe-rs download`. It
stops on a download or extraction failure, but it never checks a digest: not
of a fresh download, and not of a HEX that is already present. The recipe in
`maskfile.md` is the exact command list.

Without mask, place the HEX in the current directory and run the same
probe-rs command directly:

```sh
probe-rs download s140_nrf52_7.3.0_softdevice.hex --chip nRF52840_xxAA --format hex
```

Record the archive's source and SHA-256 with the hardware evidence; digest
verification for non-Cargo downloads is part of the open item "Supply-chain
and tooling maintenance" in
[TODO.md](../TODO.md#release-provenance-and-supply-chain). The extracted HEX
stays in the repository root, where `.gitignore` excludes it; never commit
it. Its license is described under
[Third-Party Components And Licenses](#third-party-components-and-licenses).

The SoftDevice is installed correctly when the self-test prints
`[PASS] softdevice` and the bridge logs `softdevice RAM: N bytes` followed by
`SoftDevice started`
([what healthy looks like](operations.md#what-healthy-looks-like)).

## Flashing Methods

All methods program the nRF52840 through a debug probe (on the nRF52840-DK, its
on-board debugger USB port). `--chip nRF52840_xxAA` is the probe-rs target for
every command. The Cargo runner in [.cargo/config.toml](../.cargo/config.toml)
is `probe-rs run --chip nRF52840_xxAA`, which flashes the ELF, starts it, and
streams decoded `defmt` RTT logs until you stop it.

| Command | What it builds and flashes | Use |
| --- | --- | --- |
| `mask run --release` or `mask flash` | Release `bt2usb` with the Cargo runner (the two recipes run the same command) | Normal development flashing, with logs |
| `mask run` or `mask flash-debug` | Debug-profile `bt2usb` (`opt-level = 1`, no LTO) | Debugging; not the configuration that releases use |
| `mask selftest` | Release `bt2usb-selftest` | Board bring-up; replaces the bridge until you flash it again |
| `mask rtt` | Nothing; attaches to the running target with the local release ELF | Read logs from a unit already flashed from this checkout |
| `mask softdevice` | Nordic S140 v7.3.0 HEX | Once per board; see above |

Without mask (for example in PowerShell), the equivalent Cargo commands are:

```sh
cargo run --locked --features embedded --target thumbv7em-none-eabihf --release --bin bt2usb
cargo run --locked --features embedded --target thumbv7em-none-eabihf --release --bin bt2usb-selftest
```

Local builds use `DEFMT_LOG=debug` from `.cargo/config.toml` unless the shell
already sets `DEFMT_LOG`; release artifacts use `info`. See
[collecting logs](operations.md#collecting-logs).

### Flashing A Released Artifact

The release ELF and HEX hold the same flashed bytes: the HEX is converted from
the ELF during staging. Verify whichever file you flash. The ELF is still
needed to decode logs, because the HEX carries no `defmt` string table.

```sh
# Flash the ELF and stream logs (the same command as the Cargo runner).
probe-rs run --chip nRF52840_xxAA "bt2usb-$release_tag.elf"

# Or program the HEX, reset the board, then attach with the matching ELF.
probe-rs download "bt2usb-$release_tag.hex" --chip nRF52840_xxAA --format hex
probe-rs attach --chip nRF52840_xxAA "bt2usb-$release_tag.elf"
```

These commands reuse the forms in `.cargo/config.toml` and `maskfile.md`, but
they have not been run against a published release. probe-rs option names
change between versions; check `probe-rs <command> --help` for the installed
version. Nordic's own programming tools can also program the HEX; this
repository does not use or test them.

### Erase Behavior

- The application image ends below `0x000F0000`: the linker's `FLASH` region
  stops there and fails the build if code would grow into the pairing pages.
- probe-rs erases the flash sectors it programs. Without a chip-erase option, a
  `run` or `download` is expected to leave sectors outside the image alone,
  including the SoftDevice (`0x00000000–0x00027000`) and the pairing pages
  (`0x000F0000–0x000F4000`). This has not been recorded on hardware for this
  project; confirm it once with disposable pairings and the probe-rs version you
  use.
- A full-chip erase (probe-rs's chip-erase option or its `erase` command)
  removes the SoftDevice and every stored bond. Reinstall the SoftDevice, flash
  the application, and pair every peripheral again.
- The self-test image occupies the same application region as the bridge. It
  stores and removes a scratch record (map key `0xFE`) in the pairing pages and
  only reads the saved-pairing record (key `0x01`).

## Flash A Development Unit

On a new board, complete [first flash](first-flash.md) first: it installs
SoftDevice S140 v7.3.0 and runs the self-test. To update a board that already
has SoftDevice:

1. Record the installed version/commit, board revision, and known-working
   SoftDevice version. Keep the previous ELF and its checksum for recovery.
2. Review changed pin assignments, memory reservations, and storage format.
3. Build from the intended revision using the pinned toolchain and `--locked`,
   or verify a release package as above.
4. Flash with `mask run --release`, keeping the debugger and native USB paths
   connected appropriately.
5. Check boot logs, enumeration, reconnect, input release, and the relevant
   hardware acceptance cases. Preserve the results with the artifact checksum.

The application image does not include SoftDevice. Install S140 v7.3.0 separately
on a blank board or after a full-chip erase. Normal application flashing should
preserve the reserved pairing region; verify the flashing tool's erase settings
and test this assumption for the selected workflow before relying on it. If an
update fails, follow the [operations runbook](operations.md#recovery-and-diagnostics).

## Rollback

There is no on-device rollback, anti-rollback counter, or second image slot.
Rolling back means flashing an earlier verified artifact with a probe, exactly
like an update.

1. Find the last known-good artifact in the hardware acceptance records: its
   tag or commit and the SHA-256 of the file that was flashed.
2. Prefer that archived file. A local rebuild of the same commit is not
   guaranteed to be byte-identical; reproducible-build evidence is open work in
   [TODO.md](../TODO.md#release-provenance-and-supply-chain).
3. For a release, run [Verify Before Flashing](#verify-before-flashing) with the
   old tag and its approved commit.
4. Flash it with one of the [flashing methods](#flashing-methods). Do not use a
   full-chip erase unless you intend to lose the SoftDevice and the bonds.
5. Check the boot log against
   [what healthy looks like](operations.md#what-healthy-looks-like), confirm
   that saved peripherals reconnect, and record the rollback with the hash.

### Storage Compatibility

Every build since commit `c37d568` keeps pairings in one map item (key `0x01`)
in pages 240–243, framed with magic `0xB2` and version `0x01`
([data model](data-model.md#pairing-store)). The map container is the
`sequential-storage` crate, which moved from major version 3 to 7 in commit
`dc11b4a`. The firmware version and the storage version are independent.

| Rollback target | Effect on the pairing store |
| --- | --- |
| Builds from `2479c79` onward | Same format and fail-closed loading: an unreadable or unsupported store disables writes instead of being replaced |
| Builds from `dc11b4a` up to `2479c79` | Same container and frame, but no fail-closed loading: a store they cannot fully parse may be overwritten on the next save. Builds before `f477d4c` also lack the linker guard; their `FLASH` region (868K) overlapped the pairing pages |
| Builds before `dc11b4a` | `sequential-storage` 3; whether it reads pages written by version 7 has not been checked. Treat this rollback as unsupported |
| A future firmware with a new storage version | Current firmware rejects that frame and disables writes; restore the newer firmware or accept a Factory reset |

Rolling back across a storage-version change is a migration decision, not a
routine operation. The [schema change rules](data-model.md#schema-change-rules)
require an older firmware to fail closed on a newer version.

### SoftDevice Reinstall After A Full Erase

If the board was fully erased, or the SoftDevice is missing:

1. Run `mask softdevice` (or the direct probe-rs command above).
2. Flash the chosen application artifact.
3. Pair each peripheral again. The bridge's bonds are gone; many peripherals
   also keep their side of the old bond and need to be reset or put back into
   pairing mode, which is peripheral-specific.

The [operations runbook](operations.md#recovery-and-diagnostics) lists the
recovery steps in order of impact.

## Release Gates

The workflow runs its checks on pushes to `main` or `master`, pushes of `v*`
tags, pull requests, manual dispatch, and a weekly schedule that runs the whole
workflow, not only the audit ([Delivery Flow](#delivery-flow)). Only a pushed
tag that matches the exact Cargo package version produces a **draft** release,
after every check job passes. Packaging reuses the successful embedded job's
firmware rather than rebuilding it; the package contents are listed under
[Artifact Flow](#artifact-flow). The draft keeps hardware and release review
explicit; it is not evidence that those reviews passed.

An automated tag build alone is not production approval. Before publishing a
deployment release, reviewers confirm each gate below. Each one is closed by
the [TODO.md](../TODO.md) items named with it, all of which are open today:

- Reproducible source revision, pinned toolchain and dependencies, passing
  host, embedded, and Renode checks, and recorded build and size results:
  "Reproducible firmware evidence" in
  [Release, provenance and supply chain](../TODO.md#release-provenance-and-supply-chain).
- Hardware results for the supported peripheral, host, and hub matrix,
  including sleep and wake, USB reconnect, link loss with held inputs, and both
  active slots: "Hardware compatibility baseline" in
  [Board bring-up and hardware acceptance](../TODO.md#board-bring-up-and-hardware-acceptance)
  and "Multi-device aggregation hardware acceptance" in
  [Input aggregation and delivery](../TODO.md#input-aggregation-and-delivery).
- Authenticated pairing and enrollment, key deletion and physical-access
  policy, and resolved or explicitly accepted security findings: "Authenticated
  pairing and enrollment policy" and "Refuse peer-initiated pairing on
  background reconnects" in
  [BLE central and pairing](../TODO.md#ble-central-and-pairing), and
  "Provisioning and physical key protection" in
  [Device security and provisioning](../TODO.md#device-security-and-provisioning).
- Assigned USB identity and unit-unique identification: "USB production
  identity" in [USB HID device](../TODO.md#usb-hid-device).
- Documented production hardware: "Production hardware definition" in
  [Board bring-up and hardware acceptance](../TODO.md#board-bring-up-and-hardware-acceptance).
- Memory and power margins: "Memory and endurance budget" in
  [Platform, memory and recovery](../TODO.md#platform-memory-and-recovery) and
  "Power budget and USB suspend current" in
  [UI, display and power](../TODO.md#ui-display-and-power).
- Release notes listing supported versions, compatibility limits, migrations,
  rollback constraints, checksums, and the exact SoftDevice prerequisite: the
  draft's [filled notes](#release-notes) with every `REVIEW:` line replaced.
  The supported-version line depends on "Security maintenance ownership" in
  [Release, provenance and supply chain](../TODO.md#release-provenance-and-supply-chain).
- A security contact, supported-version policy, ownership of support, and a
  dependency and license review
  ([Third-Party Components And Licenses](#third-party-components-and-licenses)):
  "Security maintenance ownership" and "Supply-chain and tooling maintenance"
  in [Release, provenance and supply chain](../TODO.md#release-provenance-and-supply-chain).
- Hosted provenance verified from a clean machine and a tested recovery
  procedure: "Hosted provenance and release recovery acceptance" in
  [Release, provenance and supply chain](../TODO.md#release-provenance-and-supply-chain).

Check an item off in [TODO.md](../TODO.md) only when its "Accept when"
criterion is met, as its
[updating rules](../TODO.md#updating-this-checklist) describe; attach hardware
or release evidence rather than inferring it from compilation.

## Third-Party Components And Licenses

bt2usb itself is licensed `GPL-3.0-only` (the `license` field in
[Cargo.toml](../Cargo.toml)); [LICENSE](../LICENSE) holds the GNU General
Public License version 3 text. The firmware also contains, or depends on,
components under other licenses:

| Component | How it reaches the device | License as declared | In the release package |
| --- | --- | --- | --- |
| bt2usb application | Source in this repository | `GPL-3.0-only`; text in `LICENSE` | Compiled into the ELF and HEX |
| `nrf-softdevice` (vendored) | Copied from upstream commit `47d6121` into `vendor/nrf-softdevice` with a local patch ([vendor notes](../vendor/nrf-softdevice/README.bt2usb.md)) | `MIT OR Apache-2.0` in its `Cargo.toml`; [LICENSE-MIT](../vendor/nrf-softdevice/LICENSE-MIT) and [LICENSE-APACHE](../vendor/nrf-softdevice/LICENSE-APACHE) are kept beside it | Compiled into the ELF and HEX |
| `nrf-softdevice-s140` | Git dependency at the same revision; Rust bindings to the S140 API | Its manifest at that revision declares `license-file = "LICENSE-NORDIC"` instead of an SPDX expression; that file is not copied into this repository | Compiled into the ELF and HEX |
| Nordic SoftDevice S140 v7.3.0 | Binary HEX that `mask softdevice` downloads from Nordic and flashes separately | Nordic's own license, which ships with Nordic's archive; it is not in this repository | Not included |
| crates.io dependencies | Resolved by [Cargo.lock](../Cargo.lock) | See below | Compiled into the ELF and HEX |

The direct firmware dependencies from crates.io (`embassy-executor`,
`embassy-nrf`, `embassy-time`, `embassy-usb`, `embassy-sync`,
`embassy-futures`, `ssd1306`, `embedded-graphics`, `cortex-m`, `cortex-m-rt`,
`embedded-hal`, `embedded-hal-async`, `sequential-storage`,
`embedded-storage-async`, `defmt`, `defmt-rtt`, `panic-probe`, `static_cell`,
and `heapless`) are each published as `MIT OR Apache-2.0` for every version of
them that `Cargo.lock` selects, as listed by the crates.io API on 2026-10-09.
The remaining transitive packages in `Cargo.lock` have not been inventoried.

The SoftDevice is a separate component obtained from Nordic: neither the
repository nor a release package contains it ([SoftDevice
Installation](#softdevice-installation)). A release package contains the
firmware, its build inputs, and the provenance files, but not `LICENSE` or any
third-party license text.

No tool checks licenses today, and this guide does not assess whether these
licenses permit distributing the firmware under `GPL-3.0-only`. That review is
part of the dependency and license gate above; a license inventory and an
automated license check are the open items "Security maintenance ownership" and
"Supply-chain and tooling maintenance" in
[TODO.md](../TODO.md#release-provenance-and-supply-chain).

## Validation Limits

Local helper tests cover version mismatch/prerelease handling, tampered or missing
files, build/source identity mismatch, input drift, unchanged firmware bytes, and
the filled release notes. `actionlint` checks workflow syntax and expressions. They
cannot issue GitHub OIDC credentials or exercise the hosted attestation/release
APIs, so whether the draft's description carries the filled notes followed by
GitHub's generated list is first seen on a hosted tag run. The first successful
tag workflow must be reviewed, and the downloaded assets must pass the commands
above, before closing the hosted-provenance validation task.

Attestations establish the producing workflow/source identity and artifact bytes.
They do not prove hardware compatibility, absence of vulnerabilities, byte-for-byte
reproducible builds, or enforcement of signatures by firmware. Production secure
boot, signed device updates, and rollback policy remain separate work.

## Related Guides

- [First flash](first-flash.md)
- [Operations](operations.md)
- [Testing](testing.md)
- [Security](security.md)
- [Hardware and memory map](hardware.md)
- [Data model](data-model.md)
- [Development](development.md)
- [Code quality](code-quality.md)
- [ADR 0008: attested draft releases](adr/0008-attested-draft-releases.md)
- [ADR 0013: pinned toolchain and mask tasks](adr/0013-pinned-toolchain-and-mask-tasks.md)
- [Task reference](../maskfile.md)
