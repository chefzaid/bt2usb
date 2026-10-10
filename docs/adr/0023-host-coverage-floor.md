# ADR 0023: Hold Host Line Coverage At A Floor As A Regression Guard

- Status: Accepted
- Date: 2026-10-10
- Amends: [ADR 0004](0004-layered-verification.md), which rejected gating on a
  coverage percentage

## Context

[ADR 0004](0004-layered-verification.md) verifies the firmware in five layers
and rejected "gate on a coverage percentage": coverage instruments only the host
library, so a threshold would reward testing what is already easy and say
nothing about the SoftDevice, USB, flash, and display shells. That reasoning
still holds. Coverage was left as a local report (`mask coverage`), with a rule
that any published figure names its commit, toolchain, and scope.

Two things changed after ADR 0004 was written:

- **The pure core grew, by design.** [ADR 0003](0003-pure-core-and-task-shell.md)
  moves every decision that can be separated from I/O into a hardware-free
  module so host tests can reach it. Through 2026-10-10 that moved the
  connection-parameter bounds, the keyboard-report decision for a
  keyboard-only Report Map, and the reconnect wake bookkeeping and
  saved-device identity out of SoftDevice-coupled files and into
  `conn_params.rs`, `hid/mod.rs`, and `reconnect.rs`. Each move is only worth
  making if the moved logic stays tested. With no measurement in CI, a later
  change could move logic in without tests, or delete tests, and every check
  would stay green.
- **A baseline now exists.** The
  [2026-10-10 validation record](../testing.md#validation-record--2026-10-10)
  measured the host library at `6e1b8b4` with cargo-llvm-cov 0.9.1 on Rust
  1.95.0: 97.59% of lines (59 of 2,452 not covered), 98.17% of regions, and
  98.67% of functions. Earlier figures in the guides (96.16%, then 97.48%)
  were each correct when written and then went stale, because nothing
  re-measured them.

The [TODO.md](../../TODO.md#verification-and-code-quality) item "Coverage and
firmware documentation in CI" (P1) asked for the `cargo llvm-cov` report as a
CI artifact and a threshold once a baseline was recorded, accepted when a
coverage drop below the threshold fails CI.

What the figure measures is fixed by the tool and the host library: the 22
source modules that [lib.rs](../../src/lib.rs) compiles for tests (`src/hid/`,
the BLE advertisement parser, connection-parameter bounds, coordinator,
reconnect table, long-read assembler, management logic, power policy, the UI
display, input, and state-machine logic, and the storage framing and record
modules), including the inline `#[cfg(test)]` modules inside them. cargo-llvm-cov
leaves `tests/integration.rs` and the separate `*_tests.rs` files out of the
report ([code quality](../code-quality.md#what-is-instrumented)).

## Decision

CI holds the host library's total line coverage at a floor, and treats the
floor as a regression guard, never as evidence.

- **Where.** A separate "Host coverage" job in
  [ci.yml](../../.github/workflows/ci.yml) runs on `ubuntu-24.04` for every
  trigger, like the other check jobs, and release packaging needs it to pass.
- **What it runs.** `cargo llvm-cov --locked --lib --tests --no-report` with
  cargo-llvm-cov 0.9.1 (the version `mask coverage-install` pins) and the
  pinned toolchain's `llvm-tools` component. One instrumented test run feeds
  every report.
- **What it publishes.** The summary table, an lcov file, and the HTML report,
  uploaded as `coverage-report-<attempt>` before the floor is checked, so a
  failing run still carries the report that shows which files lost coverage.
- **The floor.** `cargo llvm-cov report --summary-only --fail-under-lines
  "$COVERAGE_MIN_LINES"` fails the job below the floor. `COVERAGE_MIN_LINES`
  is 97, about 0.6 points under the 97.59% baseline: roughly 15 more uncovered
  lines at the same size would cross it.
- **The ratchet.** The floor rises in the commit that raises coverage enough
  to keep a similar margin. It is lowered only with the reason in the commit
  message, such as code leaving the host library, and with a new baseline in a
  validation record. It is never lowered to make a change pass.
- **What it does not mean.** Coverage is still not a verification layer.
  "Software-verified" keeps its ADR 0004 meaning (layers 1 to 3 pass), and no
  feature status, release gate, or claim in the guides rests on the figure.

## Alternatives Considered

- **Keep coverage local only (ADR 0004 as written).** No setup cost, but a
  regression is invisible until someone happens to run `mask coverage` and
  compare it with a figure that may already be stale. It does not meet the
  TODO item's acceptance criterion.
- **Publish the report in CI without a floor.** Makes the figure available on
  every run, but nobody opens the artifact of a green run, so a drop is still
  silent. Kept as the first half of this decision, not on its own.
- **A floor equal to the baseline (97.59%).** Any change that deletes a few
  covered lines, or adds a defensive branch that host tests cannot reach,
  would fail CI without losing any test. A margin keeps the floor about
  losing tests, not about every line.
- **Per-change coverage (diff coverage, or "no drop from the base branch").**
  Catches a small drop that a total floor misses, but needs either a second
  instrumented run of the base commit in every job or a hosted service such
  as Codecov or Coveralls, with a token and the report leaving the
  repository. The project's CI keeps all evidence in GitHub Actions artifacts
  ([ADR 0008](0008-attested-draft-releases.md)); this can be revisited if the
  total floor proves too coarse.
- **Per-file floors.** Would stop one module losing coverage behind another's
  gain, but 22 floors to maintain across small modules, several at 100%, would
  fail on ordinary refactors. The HTML and lcov reports already show per-file
  figures for review.
- **A region or function floor instead of lines.** Regions are finer, but
  lines are the figure every guide and validation record reports, so a line
  floor is the one a contributor can compare directly with `mask coverage`.
  Region and function figures stay in the report.
- **cargo-tarpaulin.** Linux-only, and it counts lines from its own
  instrumentation, so its figure is not comparable with the recorded llvm-cov
  baseline ([code quality](../code-quality.md#coverage)).

## Rationale

The floor protects the investment ADR 0003 makes: logic is moved into pure
modules so it can be tested, and the floor fails the build when that testing
erodes. Setting it below the baseline answers ADR 0004's objection in
practice: it does not push anyone to test easy code for a number, because
current tests already clear it, and it fails only when tests disappear or
substantial untested logic arrives. Publishing the report on every run
replaces hand-copied figures that went stale with one that is regenerated per
commit. Keeping it in its own job keeps the coverage build's instrumented
artifacts out of the host-test job's cache and lets it run in parallel with
the other checks.

## Consequences

Positive:

- Deleting tests, or adding pure logic without tests, fails CI, and the run
  carries the report that shows where coverage fell.
- Every run records the figure in its log and artifact, so a validation record
  can cite a run instead of a local measurement.
- The [code quality guide](../code-quality.md#coverage-in-ci) states the floor
  and its ratchet rules in one place.

Negative:

- One more job and one more pinned tool. cargo-llvm-cov 0.9.1 now repeats in
  `ci.yml`, `maskfile.md`, `post-create.sh`, and the development guide, with
  nothing checking that they agree
  ([ADR 0013](0013-pinned-toolchain-and-mask-tasks.md)).
- A toolchain or cargo-llvm-cov update can count regions differently and move
  the figure without any code change. Such an update re-baselines the floor in
  the same change.
- A single total can hide one module losing coverage while others gain.
- The figure still says nothing about the connection workers, security
  handler, GATT HID client, storage shell and codec, USB device, or display
  driver; those stay tracked as gaps in
  [testing](../testing.md#known-verification-gaps).

## Implementation

| Concern | Where |
| --- | --- |
| Job | `coverage` ("Host coverage") in [ci.yml](../../.github/workflows/ci.yml), 20-minute limit, `persist-credentials: false` |
| Tool install | `rustup component add llvm-tools`; `taiki-e/install-action` (SHA-pinned) with `tool: cargo-llvm-cov@0.9.1` |
| Measurement | `cargo llvm-cov --locked --lib --tests --no-report` |
| Reports | `cargo llvm-cov report` with `--summary-only` (teed to `summary.txt`), `--lcov --output-path lcov.info`, and `--html --output-dir`, all under `$RUNNER_TEMP/coverage` |
| Artifact | `actions/upload-artifact` (SHA-pinned), `coverage-report-${{ github.run_attempt }}`, `if-no-files-found: error` |
| Floor | Job-level `COVERAGE_MIN_LINES: 97`, checked last with `--fail-under-lines` |
| Release ordering | `release-package` needs `coverage` with the other check jobs |
| Policy | [Coverage in CI](../code-quality.md#coverage-in-ci) and the [testing guide](../testing.md#coverage) |
| Local equivalent | `cargo llvm-cov --locked --lib --tests --summary-only --fail-under-lines 97` |

### Verification Status

- **Implemented:** the job, its artifact, the floor, and the release ordering
  above.
- **Software-verified:** locally on 2026-10-10 with Rust 1.95.0 and
  cargo-llvm-cov 0.9.1, `cargo llvm-cov report --summary-only
  --fail-under-lines 97` exited 0 at 97.59% of lines and the same command with
  99 exited 1; the summary, lcov, and HTML reports were written; and
  actionlint 1.7.12 accepted the workflow. Hosted runs record the figure in the
  Host coverage job's log.
- **Hardware-verified:** not applicable; the check runs only on the host.

## Related

- [Code quality: coverage](../code-quality.md#coverage)
- [Testing: coverage](../testing.md#coverage)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0004: Layered verification](0004-layered-verification.md)
- [ADR 0013: Pinned toolchain and mask tasks](0013-pinned-toolchain-and-mask-tasks.md)
- [TODO: Coverage and firmware documentation in CI](../../TODO.md#verification-and-code-quality)
