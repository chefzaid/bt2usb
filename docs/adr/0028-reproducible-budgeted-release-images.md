# ADR 0028: Build Release Images Reproducibly And Hold Them To Size Budgets

- Status: Accepted
- Date: 2026-10-30

## Context

A release package carries the bridge and self-test ELF files and the bridge's
Intel HEX, with their SHA-256 in `SHA256SUMS` and `BUILD-INFO.json`
([ADR 0008](0008-attested-draft-releases.md)). The P1 item "Reproducible
firmware evidence" in
[TODO.md](../../TODO.md#release-provenance-and-supply-chain) asked to compare
artifacts from two clean environments, document what still differs, enforce
release size budgets, and make the release record identify the compiler,
dependencies, external tools, source, and artifacts. Until then
[deployment](../deployment.md#rollback) warned that a local rebuild of a
commit was not guaranteed to be byte-identical, so a rollback had to rely on
an archived file.

What a comparison on 2026-10-30 found, building commit `3a439a5` with the
pinned toolchain, `DEFMT_LOG=info`, and the release profile:

- **The flashed bytes named the build machine.** rustc writes the absolute
  path of every dependency's source file into the panic locations the code
  keeps, so the image held 31 strings under the builder's Cargo home, such as
  `/root/.cargo/registry/src/index.crates.io-…/embassy-sync-0.8.0/src/…`. A
  build under a Cargo home with a longer path produced flashed bytes
  (`llvm-objcopy -O binary`) that differed in 14,561 of the first 139,656 and
  ran 2,856 bytes longer. The application's own sources are named relative to
  the workspace (`src/main.rs`), so they did not differ.
- **The debug information named the checkout.** Two clones of the commit in
  different directories, with the same Cargo home, produced identical HEX
  files and identical ELF files once debug sections were stripped, but
  different ELF files: DWARF records the compilation directory. The ELF is
  what probe-rs flashes and what decodes the logs, and the package ships it.
- **Nothing else differed.** The `build.rs` commit stamp, the SBOM, and the
  link were already deterministic; the toolchain is pinned
  ([ADR 0013](0013-pinned-toolchain-and-mask-tasks.md)) and the lockfile is
  used with `--locked`.
- **Cargo's own fix is not available.** The profile setting `trim-paths`,
  which would remap these paths, fails on Cargo 1.95.0 with "feature
  `trim-paths` is required": it is still unstable.
- **`RUSTFLAGS` would break the link.** The linker scripts are passed as
  `rustflags` for the firmware target in `.cargo/config.toml`, and `RUSTFLAGS`
  replaces those. The per-target variable
  `CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUSTFLAGS` is joined with them instead;
  the image built with it has the same layout (vector table at `0x27000`,
  `.defmt` present).
- **No size limit existed below the linker's.** The link fails only when the
  804 KiB application flash or the 232 KiB application RAM overflows
  ([code quality](../code-quality.md#binary-size-and-memory-budgets)); the
  bridge used 140.7 KiB and 28 KiB.

## Decision

- **`release.py build` makes the release build.** It runs `cargo build
  --locked --features embedded --target thumbv7em-none-eabihf --release` with
  `DEFMT_LOG=info` (unless the caller sets it) and the per-target variable set
  to `--remap-path-prefix=<cargo home>=/cargo --remap-path-prefix=<checkout>=.`.
  The CI `embedded` job builds with it, and `mask release-build` runs it
  locally. Everyday builds (`mask build-release`, `mask run --release`) stay
  plain Cargo commands with the configuration's `debug` log level.
- **Staging refuses an image built otherwise.** `release.py stage` fails when
  either image contains the checkout path or the Cargo home path, which every
  unremapped release build does in its debug information, and names the
  command to use.
- **Size budgets per image, checked at staging.** Flash, meaning the bytes of
  every loadable segment (code, read-only data, and the initial values of
  statics), is at most 384 KiB. RAM, meaning the loadable segments at or above
  `0x20000000` (`.data`, `.bss`, and `.uninit`), is at most 64 KiB. Staging
  fails with the figures when an image is over either.
- **The record names every tool and the result.** `BUILD-INFO.json` adds
  rustc's full commit hash, host, and LLVM version (`rustc -vV`), the
  `llvm-objcopy` version that made the HEX, the two path prefixes, each
  image's flash and RAM bytes, and the budgets, next to the existing rustc and
  Cargo versions, input hashes, source commit, and artifact hashes. The SBOM
  carries each crate's lockfile checksum. CI prints `SHA256SUMS` in the
  staging step, so a local rebuild can be compared with a run without
  downloading its artifact.

## Alternatives Considered

- **`trim-paths` in the release profile.** The intended Cargo feature, but
  unstable on the pinned toolchain. Rejected until it is stable; it would
  replace the variable and the check would stay.
- **Fixed build paths**, building only in a container or under one directory
  and Cargo home. Reproducible without flags, but it ties a reproduction to
  that layout, and a developer's ordinary checkout could never match a
  release. Rejected.
- **Strip the paths after the build.** Rewriting strings in a linked image
  changes code that refers to their lengths and addresses. Rejected.
- **Compare stripped images only.** The HEX and stripped ELF already matched
  across checkouts, but the package ships and attests the full ELF, and the
  Cargo home still changed the HEX. Rejected as insufficient.
- **Remap in `.cargo/config.toml`.** Configuration values cannot read the
  environment, and the Cargo home and checkout differ per machine. Not
  possible.
- **Budgets from the linker script, or growth limits against the previous
  release.** The linker limits are already enforced and leave five times
  today's size unchecked; no release exists yet to compare with. Rejected in
  favor of fixed figures that a change can raise deliberately.
- **A CI job that builds a second time in another directory and compares.**
  It would catch a new source of nondeterminism automatically, at the cost of
  a second full release build on every push. Deferred; the staging check
  covers the cause found, and the comparison is a manual step until another
  cause appears.

## Rationale

The only differences two clean builds showed came from two path prefixes, and
remapping both is the documented rustc mechanism for exactly that; with it,
two builds under different checkouts and Cargo homes staged identical
packages, `BUILD-INFO.json` and the SBOM included. Putting the flags in one
helper that CI and `mask release-build` share, and refusing at staging any
image that still names a path, keeps the release build from silently losing
the property. The flash budget keeps the image below half the application
region, which leaves room for a second image slot if updates over USB or BLE
are added (a P2 item in TODO.md); the RAM budget leaves at least 168 KiB of
application RAM to the stack and its guard. Both are several times today's
figures, so they catch a large static or a runaway dependency, not ordinary
growth.

## Consequences

Positive:

- A release's ELF and HEX can be rebuilt from its tag and checked against
  `SHA256SUMS`, which makes rollback without the archived file possible and
  lets anyone check that a release came from its source.
- Panic locations in dependencies now read `/cargo/registry/…`, which no
  longer leaks the build machine's user name or layout.
- An image that outgrows a budget fails CI on the push that grew it, with the
  numbers in the message.

Negative and risks:

- A build made with plain Cargo differs from the release, and staging refuses
  it; reproducing a release needs `mask release-build`.
- Reproduction needs the same toolchain, host triple, lockfile, and features.
  A native Windows build would still name the Cargo home's subdirectories
  with backslashes after the prefix, so it is expected to differ from a Linux
  one (not tried); reproduce on Linux or WSL2
  ([development](../development.md)).
- The check finds only the two known prefixes. Another source of difference,
  such as a dependency's build script embedding a time or a host path outside
  them, would go unnoticed until someone compares builds.
- The budgets are judgments, not measurements: they say nothing about stack
  depth or the SoftDevice's RAM, which the open
  [memory and endurance budget](../../TODO.md#platform-memory-and-recovery)
  covers.

## Implementation

| Concern | Where |
| --- | --- |
| Remap flags, Cargo home, path check, ELF segment sizes, budgets | [scripts/release_image.py](../../scripts/release_image.py): `remap_flags`, `cargo_home`, `unmapped_paths`, `usage`, `BUDGETS`, `over_budget` |
| Build command | `build_release` and the `build` command in [scripts/release.py](../../scripts/release.py); `mask release-build` in [maskfile.md](../../maskfile.md) |
| Staging checks and record | `stage_build` in `release.py`: the path and budget checks, and the `rustc_commit`, `rustc_host`, `llvm`, `objcopy`, `path_prefixes`, `image_bytes`, and `size_budget_bytes` fields ([deployment](../deployment.md#re-running-a-tag-workflow)) |
| CI | The `Build release firmware` and staging steps of the `embedded` job in [ci.yml](../../.github/workflows/ci.yml) |
| Tests | [scripts/release_image_test.py](../../scripts/release_image_test.py) and the staging tests in [scripts/release_test.py](../../scripts/release_test.py) ([release helper tests](../testing.md#release-helper-tests)) |

### Verification Status

- **Implemented:** the build command, the staging checks, the budgets, the
  record fields, and the CI steps.
- **Software-verified:** on 2026-10-30, two clones in different directories,
  one with a copied Cargo home at another path, each built with `release.py
  build` and staged, gave byte-identical packages; staging refused an image
  from a plain `cargo build`. The CI build of the commit that made the
  change
  ([run 41054684665](https://github.com/chefzaid/bt2usb/actions/runs/41054684665))
  and a rebuild of it in a separate Linux container, from a fresh clone at
  another path and with another Cargo home, gave the same bridge ELF,
  self-test ELF, and HEX. See the
  [validation record](../validation-records.md#validation-record--2026-10-30-reproducible-release-images).
- **Hardware-verified:** not applicable; the flashed bytes are what changed,
  and the next board run uses them.

## Related

- [Deployment: reproducing a release build](../deployment.md#reproducing-a-release-build)
- [Code quality: release size budgets](../code-quality.md#release-size-budgets)
- [ADR 0008](0008-attested-draft-releases.md), the release package and its
  attestation
- [ADR 0010](0010-static-memory-layout.md), the memory map the budgets sit in
- [ADR 0013](0013-pinned-toolchain-and-mask-tasks.md), the pinned toolchain
  reproduction depends on
