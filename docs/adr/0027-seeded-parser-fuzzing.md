# ADR 0027: Fuzz The Parsers With A Seeded In-Tree Harness On The Stable Toolchain

- Status: Accepted
- Date: 2026-10-30

## Context

The bridge reads bytes it does not control from three places: a nearby
advertiser's advertising data, a paired peripheral's GATT values (the Report
Map, long reads of it, and every report notification), and its own flash,
which a power loss, a different firmware, or wear can leave in any state. The
pure modules that parse them were tested only with hand-written cases, so the
P1 item "Parser fuzzing and property tests" in
[TODO.md](../../TODO.md#verification-and-code-quality) asked for bounded fuzz
targets for HID descriptors, advertisements, report classification, and
persistence framing, accepted "when CI runs a seed corpus and scheduled fuzzing
records no panics, out-of-bounds access, excessive work, or invalid accepted
output".

Facts that shaped the decision:

- **The toolchain is pinned to stable Rust 1.95.0**
  ([ADR 0013](0013-pinned-toolchain-and-mask-tasks.md)). `cargo fuzz`
  instruments the build for libFuzzer with `-Zsanitizer`, an unstable flag that
  needs a nightly compiler (general knowledge of cargo-fuzz, not rechecked
  here), so it would test code compiled by a toolchain the firmware is never
  built with.
- **The parsers are safe Rust.** The host library is `#![forbid(unsafe_code)]`,
  so an out-of-bounds index or slice panics instead of corrupting memory, and
  integer overflow panics because host tests build with debug assertions. A
  check that catches panics already sees every memory error these modules can
  make; a sanitizer would add nothing for them.
- **What matters is wrong output, not only crashes.** A Report Map that routes
  a consumer report to the keyboard endpoint, a notification decoded with the
  wrong bytes, or a flash item that loads as different devices than were saved
  does not panic. Only a check that knows what a correct result is can find
  those.
- **The parsers are small and linear.** Each walks its input once with fixed
  capacities (16 nesting levels, 512-byte attributes, four saved devices), so
  a random search reaches deep states with plain mutation of good seeds, and a
  million inputs per target take seconds.
- **Every dependency enters the audit.** `cargo deny` checks the licenses and
  sources of the whole graph, development dependencies included, and Cargo.lock
  changes need review ([auditing](../code-quality.md#auditing)).

## Decision

- **An in-tree harness, [src/fuzz.rs](../../src/fuzz.rs),** compiled only for
  the host tests. For each target it runs a check over the seed corpus, every
  truncation of each seed, and then inputs mutated from the seeds by a seeded
  xorshift generator: bit flips, byte replacements with boundary values,
  insertions, deletions, repeated runs, truncation, and splices of two seeds,
  capped at the most the firmware can be handed. A panic in the check is
  reported with the target, the case, the input in hex, and the two settings
  that reproduce it. An input whose fastest of three reruns takes longer than
  100 ms fails as excessive work.
- **Six targets that check properties, not only survival**, in
  [src/fuzz_tests.rs](../../src/fuzz_tests.rs) and
  [src/storage_fuzz_tests.rs](../../src/storage_fuzz_tests.rs):
  - Report Maps: an accepted map has a supported input, keeps no report ID for
    a kind it lacks and never the reserved ID 0, and routes an ID only to the
    one kind that holds it.
  - Report notifications, through every classification entry point and
    against the parsed maps of the corpus: an accepted report carries exactly
    the bytes of its kind's fixed layout and serializes to its USB length, a
    numbered map routes only by its declared IDs, and an unnumbered map only to
    kinds it advertises.
  - Advertising data: the HID service check and the advertised name equal an
    independent walk of the structures, including the 32-byte cut on a
    character boundary.
  - Long attribute reads: a model of what a peer may send, so the assembled
    value is exactly the accepted fragments and ends only as ATT allows.
  - The store's framing and writer: records are the runs their prefixes
    announce, a complete frame is exactly what writing its records produces,
    and a write that does not fit leaves the frame as it was.
  - Whole store items: a device record the codec accepts is byte for byte the
    one it would write, a refused item leaves the store empty and refusing
    saves, and what a save writes for a loaded list loads back as the same
    list.
- **A reviewed seed corpus in `tests/fuzz-corpus/`**, one hex input per line
  with a comment saying what it is, plus the firmware's own USB descriptors.
  A failure the search finds becomes a seed or a unit test with the fix.
- **Two run lengths.** Every `cargo test` runs 5,000 mutated inputs per target
  with a fixed seed, so it checks the same inputs each time. A `Parser
  fuzzing` CI job on the weekly schedule and on manual dispatch runs 20 million
  per target with the workflow run ID as the seed, so each run searches new
  inputs and prints the seed. `mask fuzz` runs a million locally.

## Alternatives Considered

- **`cargo fuzz` with libFuzzer.** Coverage-guided and the usual choice, but it
  needs a nightly compiler outside the pinned toolchain, its sanitizers find
  nothing in code that forbids `unsafe`, and its corpus grows with inputs no
  one reviews. Rejected for now; the check functions take a byte slice, so a
  `fuzz/` crate on nightly could call them unchanged if the search proves too
  shallow.
- **`proptest` or `quickcheck` as a development dependency.** Structured
  strategies and shrinking, but each adds a dependency tree to Cargo.lock and
  the audit for what a page of code does, and their generators start from
  types rather than from real Report Maps and flash items, which is where the
  interesting inputs are. Rejected.
- **More hand-written cases only.** They already cover the boundaries someone
  thought of; the gap is the ones no one did, and they never check a property
  over many inputs. Rejected as the only defense; seeds and unit tests remain.
- **Fuzzing on the board or in Renode.** Real `usize` width and timing, but
  thousands of times slower, and the parsers have no target-specific code.
  Rejected; ADR 0022's corpus run in the simulation build is the place for
  target-width checks.

## Rationale

The properties are where the value is: a coverage-guided fuzzer with only a
"does not panic" check would not have found a misrouted report, and the same
properties with a random search do. The pinned stable toolchain, the absence
of `unsafe`, and the small linear parsers make a dependency-free harness enough
for the first version, and keep the fuzzed code the code the firmware builds.
Mutating reviewed seeds reaches the deep states that random bytes rarely do,
and a fixed seed in every test run with a fresh one each week gives both a
reproducible gate and a widening search.

## Consequences

Positive:

- Every pull request checks each parser against its corpus and 30,000
  mutated inputs in under a second, and every failure prints the input and
  the settings that reproduce it.
- The store targets state the persistence round trip the firmware depends on
  (what a save writes loads back unchanged), which no unit test asserted over
  arbitrary items.
- No dependency, toolchain, or Cargo.lock change.

Negative and risks:

- The search is blind: an input that needs several coordinated edits, such as
  a structure length and a multi-byte character that only together cross the
  name limit, may never be generated. The 2026-10-30 mutation check showed
  exactly that, and a seed at that boundary closed it; new boundaries need new
  seeds.
- The checks restate parts of the specification they test (the advertisement
  walk, the long-read rules), so a misreading of the specification in both
  places goes unnoticed.
- The 100 ms limit catches gross super-linear work, not a constant-factor
  slowdown, and a parser stuck in a loop is caught only by the job's timeout.
- The vendored crate's discovery code and the GATT client that calls these
  parsers are not fuzzed; they need the SoftDevice.

## Implementation

| Concern | Where |
| --- | --- |
| Harness | [src/fuzz.rs](../../src/fuzz.rs): `run`, `parse_corpus`, the mutator, `BT2USB_FUZZ_CASES` (default 5,000) and `BT2USB_FUZZ_SEED` (decimal or `0x` hex), and `MAX_INPUT_TIME`; its own three unit tests |
| Targets | `fuzz_report_maps`, `fuzz_report_notifications`, `fuzz_advertisements`, and `fuzz_long_reads` in [src/fuzz_tests.rs](../../src/fuzz_tests.rs); `fuzz_store_framing` and `fuzz_store_items` in [src/storage_fuzz_tests.rs](../../src/storage_fuzz_tests.rs); declared under `#[cfg(test)]` in [src/lib.rs](../../src/lib.rs) |
| Seed corpus | `report-map.txt`, `report.txt`, `advertisement.txt`, `long-read.txt`, and `store.txt` in [tests/fuzz-corpus/](../../tests/fuzz-corpus/), embedded with `include_str!` |
| CI | Every host test job runs the targets with `cargo test`; the `fuzz` job in [ci.yml](../../.github/workflows/ci.yml) runs `cargo test --locked --lib fuzz_ -- --nocapture` with 20 million cases and the run ID as seed, on `schedule` and `workflow_dispatch` only |
| Local | `mask fuzz` in [maskfile.md](../../maskfile.md) |

### Verification Status

- **Implemented:** the harness, six targets, the corpus, the CI job, and the
  mask recipe.
- **Software-verified:** on 2026-10-30, every target passed 20 million mutated
  inputs for each of the seeds 2 and 3, and a million for seed 1, with no
  finding; the slowest input took 17 ms, as timed on a loaded container.
  Mutants caught: records of length zero accepted by the framing (mutated case
  8), the mouse button mask removed (mutated case 8), and names cut by skipping
  characters instead of stopping (caught only after the boundary seed was
  added). See the
  [validation record](../validation-records.md#validation-record--2026-10-30-parser-fuzzing).
  The first hosted run of the `Parser fuzzing` job, a manual dispatch the
  same day
  ([run 41051151107](https://github.com/chefzaid/bt2usb/actions/runs/41051151107)),
  passed 20 million inputs per target with the seed `0x98ed6db03` in 72 s.
- **Hardware-verified:** not applicable; the parsers have no target-specific
  code.

## Related

- [Testing: parser fuzzing](../testing.md#parser-fuzzing)
- [ADR 0004](0004-layered-verification.md), the layers this joins at layer 1
- [ADR 0013](0013-pinned-toolchain-and-mask-tasks.md), the pinned toolchain
- [ADR 0022](0022-descriptor-driven-report-translation.md) (Proposed), whose
  field-table parser would join these targets
- [ADR 0025](0025-panic-lints-and-inventory.md), the panic lints whose hand-kept
  inventory these targets exercise
