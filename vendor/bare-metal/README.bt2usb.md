# bt2usb copy of bare-metal 0.2.5

This directory is the `bare-metal` 0.2.5 crate from crates.io, kept in the
repository because its upstream was deprecated and archived
([RUSTSEC-2026-0110](https://rustsec.org/advisories/RUSTSEC-2026-0110),
2026-04-23) while `cortex-m` 0.7, which the firmware cannot replace, still
depends on it. `Cargo.toml`'s `[patch.crates-io]` section points Cargo at this
copy, so no build downloads the archived crate
([ADR 0031](../../docs/adr/0031-vendored-bare-metal.md)).

## What is here

| File | Origin | SHA-256 |
| --- | --- | --- |
| `src/lib.rs` | The 0.2.5 release's `src/lib.rs`, byte for byte | `9197c65b0daec25ebb5e8c5587f82affaa35757b785a539ebb35ce91dae98b7d` |
| `LICENSE-APACHE` | The release's file, byte for byte | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| `LICENSE-MIT` | The release's file, byte for byte | `035e70219855119df4273b3c5b97543ae82e0dd60c520416e759107c602f651b` |
| `Cargo.toml` | Written for this copy | — |

The release is the crates.io archive whose checksum the lockfile held until
this copy replaced it:
`5deb64efa5bd81e31fcd1938615a6d98c82eafcbcd787162b6f63b91d6bac5b3`. To check
the three files, download `bare-metal` 0.2.5 from crates.io, verify that
checksum, and compare with `sha256sum`.

## What differs from the release

- **No build script.** The release's `build.rs` asked the `rustc_version`
  crate for the compiler version and set the cfg `unstable_const_fn` for
  rustc older than 1.31. `src/lib.rs` in 0.2.5 never reads that cfg, and the
  pinned toolchain is far newer, so the script did nothing. Leaving it out
  removes a build script and three build-time crates from the graph:
  `rustc_version` 0.2.3, `semver` 0.9.0, and `semver-parser` 0.7.0.
- **Manifest.** `Cargo.toml` keeps the name, version, license, and the
  `const-fn` feature that `cortex-m` enables (it has never done anything in
  0.2.5), states edition 2015 as the release implies, and sets
  `publish = false`. The release's CI files, changelog, and README are not
  copied.

The source is not edited, so the firmware contains the same code as before.

## What it provides

`cortex-m` 0.7 re-exports three items from it in `cortex_m::interrupt`:
`CriticalSection` (the token `interrupt::free` hands its closure), `Mutex`
(a cell readable only with that token), and the `Nr` trait (the old interrupt
number trait, kept for peripheral crates older than `InterruptNumber`). The
crate also defines `Peripheral`, which nothing in the firmware uses. All four
are zero-cost wrappers: 101 lines, no dependencies, no code of their own in
the image. [vendor_bare_metal.rs](../../tests/vendor_bare_metal.rs) tests them
on the host.

`Mutex` is sound only on single-core systems, as its documentation says; the
nRF52840 is one.

## When to delete it

Delete this directory, the `[patch.crates-io]` entry, the dev-dependency, and
the test when `Cargo.lock` no longer needs `bare-metal`: that is, when a
`cortex-m` release without it is adopted. Upstream's removal is a breaking
change planned for `cortex-m` 0.8 (rust-embedded/cortex-m issue 651 and draft
pull request 656, read 2026-11-14), and Embassy and `nrf-softdevice` must
accept that release first. Do not update this copy: upstream is archived, so
there is nothing newer to take.

Advisory tools match crates.io packages, so `cargo deny` does not report
advisories against this copy; the audit therefore needs no ignore for it,
and a future advisory against `bare-metal` would not fail CI either. The
[supply-chain notes](../../docs/security.md#supply-chain) record that limit.
