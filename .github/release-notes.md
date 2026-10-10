<!--
Filled from .github/release-notes.md by `python scripts/release.py notes` in
the tag workflow; GitHub appends its generated list of changes below. Before
publishing the draft, replace every line that starts with "REVIEW:", check the
generated list, and delete this comment (docs/deployment.md#release-notes).
-->

**bt2usb ${version}** (${kind}) built from commit
[`${short_commit}`](https://github.com/${repository}/commit/${commit}) by
workflow run [${run_id}](https://github.com/${repository}/actions/runs/${run_id}).
CI creates this draft after every check passes; a passing build is not
hardware acceptance.

## Supported Versions

REVIEW: Which releases receive fixes once this one is published (the policy in SECURITY.md), and a link to the hardware acceptance record for this exact build.

## SoftDevice Prerequisite

This firmware runs only on Nordic SoftDevice **${softdevice}**
(`${softdevice_hex}`), which the release does not contain. The application is
linked to start at `${app_flash_start}` in flash and `${app_ram_start}` in RAM,
the memory map of that exact version, and its SoftDevice bindings are written
for it, so never combine it with another SoftDevice version or variant.
Install the SoftDevice once per board with `mask softdevice` or as
[SoftDevice installation](https://github.com/${repository}/blob/${tag}/docs/deployment.md#softdevice-installation)
describes, and again after any full-chip erase.

## Compatibility Limits

- Board: nRF52840 with the nRF52840-DK pin map; another board needs the
  changes in
  [porting](https://github.com/${repository}/blob/${tag}/docs/hardware.md#porting-to-another-nrf52840-board).
- Peripherals: Bluetooth LE HID over GATT keyboards and mice, at most
  ${max_connections} connected at once and ${max_paired} saved; saving another
  evicts the oldest.
- USB: one full-speed composite device with boot keyboard, boot mouse, and
  consumer-control interfaces, under VID `${usb_vid}` and PID `${usb_pid}`: a
  pid.codes development identity that is not assigned to this product.
- REVIEW: The peripherals, hosts, monitor hubs, and KVMs this build was tested with, and any limit found while testing it.

The limits every build has today, including Just Works pairing, are listed
under
[current technical boundaries](https://github.com/${repository}/blob/${tag}/docs/features.md#current-technical-boundaries).

## Pairing Storage And Migrations

Saved devices and their bonds live in flash pages ${storage_pages} (from
`${storage_start}`), in one item framed with magic `${storage_magic}` and
storage version `${storage_version}`
([pairing store](https://github.com/${repository}/blob/${tag}/docs/data-model.md#pairing-store)).
Firmware that finds a version it does not know keeps the item and refuses to
save until a factory reset, instead of overwriting it.

REVIEW: "No migration: storage version unchanged since <previous tag>", or the migration this release performs on first boot and whether an older release can still read the result.

## Rollback Constraints

There is no on-device rollback or second image slot. Rolling back means
flashing an earlier verified release with a debug probe, without a full-chip
erase, so the SoftDevice and the saved devices stay. An earlier release that
does not support storage version `${storage_version}` cannot use the saved
devices this one wrote; check
[storage compatibility](https://github.com/${repository}/blob/${tag}/docs/deployment.md#storage-compatibility)
before rolling back.

REVIEW: The oldest release this one can roll back to without a factory reset.

## Checksums

SHA-256 of each release file, as listed in `SHA256SUMS`, which the provenance
attestation signs:

```text
${checksums}
```

Verify the attestation against the commit your review approved, then every
file, before flashing
([verify before flashing](https://github.com/${repository}/blob/${tag}/docs/deployment.md#verify-before-flashing)):

```sh
approved_commit=REPLACE_WITH_THE_COMMIT_YOUR_REVIEW_APPROVED
gh release download ${tag} --repo ${repository} --dir ${tag}
cd ${tag}
gh attestation verify SHA256SUMS --repo ${repository} \
  --signer-workflow ${repository}/.github/workflows/ci.yml \
  --source-ref refs/tags/${tag} --source-digest "$$approved_commit" \
  --deny-self-hosted-runners
sha256sum --strict --check SHA256SUMS
```

## Build

`${rustc}`, target `thumbv7em-none-eabihf`, release profile with
`DEFMT_LOG=${defmt_log}`. `BUILD-INFO.json` records the full build identity.
