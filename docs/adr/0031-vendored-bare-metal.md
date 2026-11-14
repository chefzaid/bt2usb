# ADR 0031: Keep The Archived bare-metal Crate As An In-Tree Copy Until cortex-m Drops It

- Status: Accepted
- Date: 2026-11-14

## Context

After [ADR 0030](0030-in-tree-ssd1306-driver.md) removed `proc-macro-error`,
one unmaintained crate was left in `Cargo.lock`, and one advisory ignore in
`deny.toml`: `bare-metal 0.2.5`, `RUSTSEC-2026-0110`. The advisory is
informational: on 2026-04-23 the crate was deprecated and its repository
archived, and it points users of `Mutex` and `CriticalSection` to the
`critical-section` crate. No patched version exists or will.

bt2usb does not use `bare-metal`. `cortex-m` 0.7 does, with one line:
`pub use bare_metal::{CriticalSection, Mutex, Nr};` in its `interrupt`
module. Those three names are part of `cortex-m`'s public API (the token
`interrupt::free` passes its closure, a cell readable only with that token,
and the interrupt number trait that predates `InterruptNumber`), so taking
them out is a breaking change for `cortex-m`:

- `cortex-m` 0.7.10, the latest release, and the repository's default branch
  still depend on `bare-metal` 0.2 (crates.io index and the crate's manifest,
  read 2026-11-14). No 0.8 is published.
- Upstream tracks the removal in rust-embedded/cortex-m issue 651
  ("Deprecation of `bare-metal`", opened 2026-04-24) and draft pull request
  656 ("Remove bare-metal dependency", opened 2026-04-27, last updated
  2026-11-12), which includes pull request 654; that one and its
  alternative, 704, change the interrupt number traits, and their author
  describes both as breaking.
- `cortex-m` sets `links = "cortex-m"`, so an image holds one version, and
  `embassy-executor`, `embassy-nrf`, `embassy-hal-internal`, `nrf-pac`,
  `nrf-softdevice`, and `panic-probe` all require 0.7. Even after a 0.8
  release, bt2usb can adopt it only when those crates do.

So the crate cannot leave the dependency graph from bt2usb's side for some
time. What bt2usb can decide is where its source comes from. The crate is
small enough to read in a minute: one file of 101 lines with no
dependencies, defining four zero-cost wrappers. The 0.2.5 release also
carries a build script that calls `rustc_version` to set a cfg for rustc
older than 1.31; the release's `src/lib.rs` never reads that cfg, so on any
current toolchain the script and its three build-time crates
(`rustc_version` 0.2.3, `semver` 0.9.0, `semver-parser` 0.7.0) run on every
build machine and do nothing.

## Decision

Keep a copy of `bare-metal` 0.2.5 in [vendor/bare-metal](../../vendor/bare-metal/README.bt2usb.md)
and point Cargo at it with `[patch.crates-io]`, so that no build downloads
or runs code from the archived crate, and drop the advisory ignore.

- **Source unchanged.** `src/lib.rs` and the two license files are the
  crates.io release's files byte for byte; the README beside them records
  their SHA-256 digests and the release archive's checksum. The firmware
  therefore contains the same code as before.
- **No build script.** The copy leaves out `build.rs` and its
  `rustc_version` build-dependency, which removes three crates from
  `Cargo.lock` and one build script from every build.
- **Manifest.** The copy's `Cargo.toml` keeps the name, version 0.2.5, the
  license, and the `const-fn` feature `cortex-m` enables, and sets
  `publish = false`.
- **Tests.** [vendor_bare_metal.rs](../../tests/vendor_bare_metal.rs) has
  four host tests of the copy: the token and the mutex add no data, the
  mutex gives access only with a token, and the interrupt number trait and
  the peripheral wrapper return what they were given.
- **Audit.** `deny.toml` ignores no advisory. `cargo deny` matches advisories
  to crates.io packages, so it does not report `RUSTSEC-2026-0110` for the
  copy; the same rule means a later advisory against `bare-metal` would not
  fail CI. That limit is recorded in `deny.toml`, the copy's README, and the
  [supply-chain notes](../security.md#supply-chain).
- **End of life.** The copy is deleted, with the patch entry, the
  dev-dependency, and the test, when a `cortex-m` release without
  `bare-metal` is adopted. It is never updated: upstream is archived.

## Alternatives Considered

- **Keep the advisory ignore** ([ADR 0030](0030-in-tree-ssd1306-driver.md)'s
  first decision, which this record replaces). It is honest and costs
  nothing, and a different advisory against `bare-metal` would still fail
  CI. But every build keeps downloading an archived crate and running its
  build script and three more old build-time crates, and the audit keeps a
  standing exception whose removal depends on a breaking upstream release
  and on Embassy adopting it.
- **Vendor `cortex-m` with the three items defined inline.** `bare-metal`
  would leave the lockfile entirely. But the copy would be the whole
  Cortex-M support crate (register access, assembly, the critical-section
  implementation) instead of 101 lines; Dependabot could no longer propose
  its updates, such as 0.7.10's fixes; and the advisory check would stop
  covering `cortex-m` itself, a crate where an advisory would matter.
- **Patch `cortex-m` to upstream's draft pull request 656.** Unreleased,
  unreviewed code in the crate under the executor and every interrupt, with
  API changes its dependents have not adopted.
- **Trim the copy to the three re-exported items.** A smaller file, but no
  longer the release's file, so the copy could not be checked against the
  release by digest. `Peripheral` costs nothing in the image.
- **Replace `cortex-m`.** Not possible while Embassy, `nrf-pac`, and
  `nrf-softdevice` require it.

## Rationale

The risk an unmaintained crate carries is that its code changes hands or
that nobody fixes it. A frozen in-tree copy of 101 lines removes the first
and makes bt2usb the maintainer for the second, at no cost in function: the
source is the same bytes. Unlike a vendored `cortex-m`, there is no upstream
left to fall behind. The copy also does something the ignore did not: it
takes a dead build script and three old build-time crates off every build
machine. What it gives up is the audit's view of one archived 101-line
crate, which is stated wherever the copy is described.

## Consequences

Positive:

- `Cargo.lock` selects no crate with an advisory, and `cargo deny` runs with
  no ignore, so the audit has no standing exception.
- `rustc_version`, `semver` 0.9.0, and `semver-parser` 0.7.0 leave the
  graph, and no build runs `bare-metal`'s build script.
- The firmware's functions are unchanged: release builds with and without
  the copy have the same symbols with the same sizes.
- The three items `cortex-m` re-exports now have host tests.

Negative:

- A second vendored crate to account for in reviews, the SBOM, and the
  license notes, although it never changes.
- `cargo deny` would not report a future advisory against `bare-metal`.
- `Cargo.lock` still lists `bare-metal` 0.2.5, now without a registry
  source, and the SBOM lists it as vendored source: the dependency is owned,
  not gone, until `cortex-m` drops it.
- If the `[patch.crates-io]` entry is removed, every `--locked` command
  fails on the stale lockfile, and once the lockfile is regenerated the
  audit job fails on `RUSTSEC-2026-0110`; both are the intended alarm.

## Implementation

| Concern | Where |
| --- | --- |
| The copy and its provenance | [vendor/bare-metal](../../vendor/bare-metal/README.bt2usb.md): `src/lib.rs`, `Cargo.toml`, the license files |
| Patch entry and test dependency | `[patch.crates-io]` and `[dev-dependencies]` in [Cargo.toml](../../Cargo.toml); [Cargo.lock](../../Cargo.lock) |
| Host tests | [vendor_bare_metal.rs](../../tests/vendor_bare_metal.rs) |
| Audit configuration | [deny.toml](../../deny.toml), with no `ignore` list |
| SBOM | `release_sbom.py` lists a path dependency as vendored source at its repository path ([software bill of materials](../deployment.md#software-bill-of-materials)) |

### Verification Status

- **Implemented:** everything above.
- **Software-verified:** the four tests of the copy and the rest of the host
  suite pass; Clippy with `-D warnings` passes for the host, the firmware,
  the firmware with `log-sensitive-data`, and the simulation; release builds
  of the bridge and the self-test with and without the copy have identical
  symbol tables apart from hash suffixes (same names, kinds, and sizes); the
  three Renode test cases pass; `cargo deny --locked check` passes with no
  ignore, and fails on `RUSTSEC-2026-0110` when the patch entry is removed
  and the lockfile regenerated
  ([2026-11-14 record](../validation-records.md#validation-record--2026-11-14-in-tree-bare-metal-copy)).
- **Hardware-verified:** not applicable to the copy itself; the firmware's
  code is unchanged, and its board evidence is the first-flash record.

## Related

- [Code quality: auditing](../code-quality.md#auditing)
- [Security: supply chain](../security.md#supply-chain)
- [Deployment: third-party components and licenses](../deployment.md#third-party-components-and-licenses)
- [ADR 0007: Vendored nrf-softdevice patch](0007-vendored-softdevice-patch.md)
- [ADR 0030: In-tree SSD1306 driver](0030-in-tree-ssd1306-driver.md)
- [TODO.md: Replace unmaintained transitive dependencies](../../TODO.md#release-provenance-and-supply-chain)
