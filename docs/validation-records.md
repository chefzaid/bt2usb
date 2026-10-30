# Validation Records

Dated records of the software checks run for each change, newest first. Each
names the commit it covers, the environment, and every result, including
checks skipped or not run; nothing here is board evidence unless it says so.
How to run each check is in the [testing guide](testing.md), and board
results go in a [hardware-result issue](../.github/ISSUE_TEMPLATE/hardware-result.md).
The documentation checker skips these records' values, since they describe
the tree as it was.

## Validation Record — 2026-10-30, Coordinator Task Fault Tests

This record covers the commit that checks off the P1 item "Coordinator task
fault tests" in [TODO.md](../TODO.md#verification-and-code-quality): the
coordinator task's loop moves from `ble_task`, `manage_devices`,
`handle_slot_event`, and `execute_action` in `src/ble/multi_conn.rs` into
`run` in the host-compiled `src/ble/coordinator_loop.rs`, generic over a
`CoordinatorIo` trait; `multi_conn.rs` keeps `ble_task` and the firmware's
`CoordinatorIo` (`Shell`). Eight fault tests run the production loop on a
scripted bench, with the test playing both connection workers
([coordinator loop fault tests](testing.md#ble-coordination-and-discovery)).
Writing them found one defect, fixed in the same commit: a slot freed by a
quiescence acknowledgement kept the keys of a pairing it had not saved
([FIXME](../TODO.md#fixme)); `on_slot_quiesced` now returns
`DiscardUnsavedBond` for it, in the firmware and in the simulation. The
firmware's behavior is otherwise meant to be unchanged; the checks ran
locally on Linux in a container, and nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| `mask ci` steps | The recipe's script, as in the HFXO record | Passed after this record was added; 482 unit tests (8 new in `coordinator_loop_tests.rs`, 1 in `coordinator_attempt_tests.rs`) and the 13 integration, glyph, pairing-permission, and portal tests |
| Same loop as before | Review of the old `ble_task` and its helpers against `run` and `Shell` | The loop is the same code with each I/O call behind a `CoordinatorIo` method that `Shell` implements with the code it replaced: the store load and bond import, the saved devices with keys taken for power-up, the `select` over the two channels, the sends to the UI and the slots, `scanner::scan`, `bonder().same_peer`, the saved-device list, the store lookup and identity match for Forget, the store write and bonder update for a Forget or reset, the whole `PersistDevice` arm, and `discard_unsaved`. The scan snapshot is an empty list where it was `None`, which `plan_connect` treats the same. The one change is the acknowledgement's `on_slot_quiesced` |
| Mutants | Two deliberate breaks of `coordinator_loop.rs`, each reverted | Handling a targeted slot's events during the barrier failed the Forget test; freeing a quiesced slot without `on_slot_quiesced` failed the stale-acknowledgement test |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only`, cargo-llvm-cov 0.9.1 | 98.66% of lines (63 of 4,717 missed), 98.51% of regions, 98.64% of functions; `coordinator_loop.rs` 98.43% of lines. The summary lists 40 files, of which the fuzzing harness and the two fault-test benches are test code ([what is instrumented](code-quality.md#what-is-instrumented)) |
| Renode scenario | `renode-test renode/bt2usb-sim.robot`, Renode on `PATH` | Not run: Renode could not download the nRF52840 SVD from `dl.antmicro.com`, which this environment's network policy refuses; the CI Renode job runs it. The simulation's log lines do not change |
| Board/radio/USB acceptance | Physical hardware | Not run; the GAP lock between scans and connects is still checked only on hardware |

## Validation Record — 2026-10-30, Slot Worker Fault Tests

This record covers the commit that checks off the P1 item "Async task fault
tests" in [TODO.md](../TODO.md#verification-and-code-quality): the slot
worker's loop moves from `connection_slot_task` in `src/ble/slot_worker.rs`
into `run` in the host-compiled `src/ble/slot_loop.rs`, generic over a
`SlotIo` trait, with `SlotCommand`, `SlotEvent`, and `SlotOutcome` made
generic over the address beside it; `slot_worker.rs` keeps the firmware's
`SlotIo` (`Radio`), and `embassy-futures` becomes a dependency of every build
so the loop's `select` compiles on the host. Ten fault tests run the
production loop against a scripted radio
([slot worker fault tests](testing.md#test-map)). The firmware's behavior is
meant to be unchanged; the checks ran locally on Linux in a container, and
nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| `mask ci` steps | The recipe's script, as in the HFXO record | Passed after this record was added; 473 unit tests (10 new in `slot_loop_tests.rs`) and the 13 integration, glyph, pairing-permission, and portal tests |
| Same loop as before | Review of the old `connection_slot_task` against `run` and `Radio` | The loop body is the same code with each radio call behind a `SlotIo` method that `Radio` implements with the call it replaced: `cmd_rx.receive()`, `slot_event_tx.send()`, the 500 ms `Timer`, `bonder().bond_for_address()`, the five `scanner` functions, and `connect_and_run_secure`. The three log lines are `Note`s that `Radio` prints with the same text. The LED receiver is taken once per slot, as before |
| Fault tests that failed first | The first run of the new tests | Five failed on expectations, not on the loop: a failed attempt's UI events are the error followed by `Disconnected` (`on_slot_error` sends both), a quiet retry scans for the address the device was last seen at, and a failed takeover drops the selection's unsaved pairing. The tests now assert those |
| `Cargo.lock` | `git diff` | Unchanged; `embassy-futures` 0.1 was already locked |
| Board/radio/USB acceptance | Physical hardware | Not run; the coordinator task's own loop and the GAP lock between scans and connects are still checked only on hardware |

## Validation Record — 2026-10-30, Security Handler And USB Checks

This record covers the commit that checks off the P1 item "Host tests for the
I/O shells" in
[TODO.md](../TODO.md#verification-and-code-quality): the security handler's
per-link flags move from `src/ble/bonder.rs` into `LinkFlags` in the
host-compiled `src/ble/bond_table.rs`, the length check of the host's LED
output report moves from `set_report` in `src/usb/host_requests.rs` into
`KeyboardLeds::from_output_report` in `src/hid/keyboard.rs`, and the
[testing guide](testing.md#modules-without-host-tests) now says, for every
shell, which of its decisions are host-tested and why the code left is not.
Together with the three records below, the item moved decisions out of five
shells with 22 new host tests. The behavior is meant to be unchanged; the
checks ran locally on Linux in a container, and nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| `mask ci` steps | The recipe's script, as in the HFXO record | Passed after this record was added; 463 unit tests (2 new for `LinkFlags`, 1 for `from_output_report`) and the 13 integration, glyph, pairing-permission, and portal tests |
| Same behavior as before | Review of the old `LinkFlags` in `bonder.rs` and the old `let &[byte] = data` check against the new code | `LinkFlags` is the same code over a generic address, its `RefCell` now held by `Bonder`; `from_output_report` accepts exactly one byte and masks it with `from_byte`, as before |
| Shells left | Reading each shell for decisions that build without the firmware target | None found that is not covered: what remains awaits SoftDevice, `embassy-usb`, or `embassy-nrf` types and calls. The OLED retry after a bus error is recorded as the one error path no automated check reaches |
| Board/radio/USB acceptance | Physical hardware | Not run |

## Validation Record — 2026-10-30, Persistence Report

This record covers the commit that moves what the user is told after a
connected device's store write out of `execute_action` in
`src/ble/multi_conn.rs` into `persist_report` in the host-compiled
`src/ble/management.rs`, together with the `Stored` outcome it reads (now
`management::Stored<A>`, which `src/storage.rs` names for the SoftDevice
address), the third step of the P1 item "Host tests for the I/O shells" in
[TODO.md](../TODO.md). The behavior is meant to be unchanged; the checks ran
locally on Linux in a container, and nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| `mask ci` steps | The recipe's script, as in the HFXO record | Passed after this record was added; 460 unit tests (2 new in `management.rs`) and the 13 integration, glyph, pairing-permission, and portal tests |
| Same decisions as before | Review of the old `match` on the save result and `Stored` against `persist_report` | Each arm maps one to one: after a save, `ReplacedBond` and `RemovedOldest` give their notices and anything else nothing; after a failed save, a saved, replacing, or evicting add gives `NotPersisted`, and an unchanged or refused one `StorageFailed` |
| Simulation | `src/sim_ble.rs` | Unchanged: it keeps its own store write, which reports `StorageFailed` for any failed save and sends no notices ([modules without host tests](testing.md#modules-without-host-tests)) |
| Board/radio/USB acceptance | Physical hardware | Not run |

## Validation Record — 2026-10-30, Connection Slot Decisions

This record covers the commit that moves a connection slot's decisions out of
`src/ble/slot_link.rs` and `src/ble/slot_worker.rs` into the host-compiled
`src/ble/slot_policy.rs`, the second step of the P1 item "Host tests for the
I/O shells" in [TODO.md](../TODO.md): `securing_error`, the error a link that
would not encrypt fails with, and `after_attempt`, what the slot does once an
attempt ends ([modules without host tests](testing.md#modules-without-host-tests)).
The workers' behavior is meant to be unchanged; the checks ran locally on
Linux in a container, and nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| `mask ci` steps | The recipe's script, as in the record below | Passed after this record was added; 458 unit tests (7 new in `slot_policy_tests.rs`) and the 13 integration, glyph, pairing-permission, and portal tests |
| Same decisions as before | Review of the old `match` blocks in `secure_and_discover` and `connection_slot_task` against the new functions | Each arm maps to one case: an encrypted link or completed pairing passes; refused keys give `KeysRejected`, a failed pairing `PairingFailed`, a background reconnect without keys `KeysMissing`, anything else `ConnectFailed`; a closed link reconnects or, without keys, reports `KeysMissing`; a background `ConnectFailed` retries quietly; a takeover makes the user's connection; any other failure reports and resumes the replaced retry only while its keys remain and the error does not need a new pairing. A superseded attempt is still handled in the worker, as before. The log lines the guides quote are unchanged |
| Board/radio/USB acceptance | Physical hardware | Not run; connecting, losing, and re-pairing real peripherals still checks the procedures left in the two shells |

## Validation Record — 2026-10-30, GATT HID Client Decisions

This record covers the commit that moves the GATT HID client's decisions out
of `src/ble/hid_client.rs` into the host-compiled `src/ble/hid_gatt.rs`, the
first step of the P1 item "Host tests for the I/O shells" in
[TODO.md](../TODO.md): `report_use` for each Report characteristic,
`report_map_step` and `report_map` for the Report Map long read, and
`notification` for the notification filter
([modules without host tests](testing.md#modules-without-host-tests)). The
client's behavior is meant to be unchanged; the checks ran locally on Linux in
a container, and nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| `mask ci` steps | The recipe's script: formatting, Clippy for host, embedded, embedded with `log-sensitive-data`, and simulation, host tests, rustdoc for all five builds, the Markdown checks, release and simulation builds | Passed after this record was added; 451 unit tests (11 new in `hid_gatt_tests.rs`) and the 13 integration, glyph, pairing-permission, and portal tests |
| Same decisions as before | Review of the old `subscribe_all`, `read_report_map`, and `on_hvx` against the new functions | Each branch maps one to one: no CCCD gives the LED output or nothing, a CCCD on an Output or Feature reference is ignored, a numbered map skips an unresolved input, read errors keep their tags, and notifications over 32 bytes or from other handles are dropped. The kind no longer passes through `filter(ReportReference::is_input)`, which could only drop a reference the previous check had already returned on |
| Notification fuzz target | `fuzz_report_notifications` | Its length limit is now `MAX_REPORT_LEN` from `hid_gatt.rs` instead of a copied 32 |
| Release image | `release.py build` | Bridge 139,404 bytes of flash (68 fewer than at `3a439a5`) and 28,108 of RAM, within the budgets |
| Board/radio/USB acceptance | Physical hardware | Not run; connecting a real keyboard and mouse still checks the SoftDevice calls left in `hid_client.rs` |

## Validation Record — 2026-10-30, Reproducible Release Images

This record covers the commit that checks off the P1 item "Reproducible
firmware evidence" in
[TODO.md](../TODO.md#release-provenance-and-supply-chain): `release.py build`
with the remapped build paths, the staging checks for build paths and size
budgets, the new `BUILD-INFO.json` fields, `mask release-build`, and the CI
build and staging steps
([ADR 0028](adr/0028-reproducible-budgeted-release-images.md),
[reproducing a release build](deployment.md#reproducing-a-release-build)). The
checks ran locally on Linux in a container; the comparison builds used local
clones, and nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Before the change: two checkouts | Clones of `3a439a5` in two directories of different depth, one Cargo home, `cargo build --release` with `DEFMT_LOG=info` | The bridge and self-test HEX files, and the ELF files with debug sections stripped, were identical; the full ELF files differed |
| Before the change: two Cargo homes | The same build with a copy of the Cargo home at another path | The flashed bytes (`llvm-objcopy -O binary`) differed in 14,561 of the first 139,656 and ran 2,856 bytes longer; the image held 31 paths under the Cargo home, all in dependencies' panic locations |
| `trim-paths` | `trim-paths = "all"` in the release profile, Cargo 1.95.0 | Refused: "feature `trim-paths` is required" |
| After the change: two environments | The two clones, each with this change committed locally as `f4d6d1d`, one with each Cargo home, each built with `release.py build` and staged with `release.py stage` | Every staged file byte-identical: `bt2usb.elf` `7ce7fed8…`, `bt2usb-selftest.elf` `884ccc8a…`, `bt2usb.hex` `7564fcd9…`, `BUILD-INFO.json`, and the SBOM; neither image contains a build path, and the layout is unchanged (vector table at `0x27000`, `.defmt` present) |
| Staging refuses a plain build | `cargo build --release` in one clone, then `release.py stage` | Refused, naming the checkout and `/root/.cargo`; nothing written |
| Recorded sizes | `image_bytes` in the staged `BUILD-INFO.json` | Bridge 139,472 bytes of flash and 28,108 of RAM; self-test 69,116 and 12,704; budgets 393,216 and 65,536 |
| Script tests and lint | `python3 -m unittest discover -s scripts -p '*_test.py'`; `scripts/lint_scripts.py` and Ruff 0.16.9 on the two new files | Passed: 64 tests, 30 of them release-helper (5 new); lint clean, including the `release-build` recipe |
| `mask ci` steps | The recipe's script | Passed after this record was added |
| actionlint | Not installed here | Not run on the changed `embedded` steps |
| Hosted comparison | GitHub Actions `ubuntu-24.04` against this container | Not run before the commit. The push run [41054684665](https://github.com/chefzaid/bt2usb/actions/runs/41054684665) on that commit, `2967978`, printed `bt2usb.elf` `895a562f…`, `bt2usb-selftest.elf` `5dc66fb3…`, and `bt2usb.hex` `695d7cb0…`; a `release.py build` of the same commit in a fresh clone in this record's container, at another checkout path and with another Cargo home, gave the same three hashes |
| Board/radio/USB acceptance | Physical hardware | Not applicable: the code is unchanged, only the paths in its panic locations and debug information |

## Validation Record — 2026-10-30, HFXO Request

This record covers the commit that fixes the FIXME "USB never requested the
crystal it clocks from" in [TODO.md](../TODO.md#fixme): `request_hfxo` in
`src/sd_setup.rs` requests the high-frequency crystal through the SoftDevice
right after `Softdevice::enable` in the bridge and the self-test, and the
self-test reports it as the `hfxo` stage
([Clocks And Radio](hardware.md#clocks-and-radio)). The checks ran locally on
Linux in a container on the working tree just before that commit; nothing ran
on a board, so USB stability with the request stays an open hardware item.

| Check | Environment | Result |
| --- | --- | --- |
| `mask ci` steps | The recipe's script: formatting, Clippy for host, embedded, embedded with `log-sensitive-data`, and simulation, host tests, rustdoc for all five builds, the Markdown checks, release and simulation builds | Passed; 440 unit tests and the 13 integration, glyph, pairing-permission, and portal tests unchanged |
| The request is in both images | `llvm-objdump -d` on the release `bt2usb` and `bt2usb-selftest`, counting `svc #0x42` (`sd_clock_hfclk_request`) and `svc #0x44` (`sd_clock_hfclk_is_running`) | One of each in both images; none of `svc #0x42` in the bridge built from the previous commit |
| Release image size | `llvm-size` on the release `bt2usb` | Text grew from 138,848 to 139,076 bytes (+228); data and bss unchanged |
| `unsafe` inventory | `grep -rnw unsafe src build.rs tests` | Fifteen lines, as [code quality](code-quality.md#inventory) now states with row 14 |
| Board/radio/USB acceptance | Physical hardware | Not performed: the start time, the self-test's `hfxo` stage, and USB stability with the radio idle and active are [first-flash checks](first-flash.md#8-extended-acceptance-optional) |

## Validation Record — 2026-10-30, Parser Fuzzing

This record covers the commit that checks off the P1 item "Parser fuzzing and
property tests" in [TODO.md](../TODO.md#verification-and-code-quality): the
harness in `src/fuzz.rs`, six fuzz targets in `src/fuzz_tests.rs` and
`src/storage_fuzz_tests.rs`, the seed corpus in `tests/fuzz-corpus/`, the
weekly and manual `Parser fuzzing` CI job, and `mask fuzz`
([ADR 0027](adr/0027-seeded-parser-fuzzing.md),
[parser fuzzing](testing.md#parser-fuzzing)). The checks ran locally on Linux
in a container on the working tree just before that commit; nothing ran on a
board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 440 unit tests (9 new: six targets at 5,000 mutated inputs each and three harness tests), 3 integration tests, 3 glyph-table tests, 2 pairing-permission tests, and 5 vendored-portal tests; the library tests took 0.13 s |
| Long fuzzing | `cargo test --locked --lib fuzz_` with `BT2USB_FUZZ_CASES` 1,000,000 for seed 1 and 20,000,000 for seeds 2 and 3 | Passed, no finding in 246 million inputs; 20 million per target took 74 s for all six. The slowest input took 17 ms, on a container shared with other work |
| Mutants | Each change made by hand, run against its target, and reverted | Caught: a framing that accepts zero-length records (mutated case 8 of `store-framing`) and a mouse decoder without its button mask (mutated case 8 of `report`). Not caught in a million inputs: a name cut that skips a character that does not fit instead of stopping; caught by the seed added for it, a 31-byte name, a two-byte character, and one more character |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only`, cargo-llvm-cov 0.9.1 | 98.93% of lines, 98.49% of functions, 98.73% of regions; `fuzz.rs` misses its environment parsing and the slow-input rerun |
| `mask ci` steps | The recipe's script: formatting, Clippy for host, embedded, embedded with `log-sensitive-data`, and simulation, host tests, rustdoc for all five builds, the Markdown checks, release and simulation builds | Passed |
| Script tests and lint | `python3 -m unittest discover -s scripts -p '*_test.py'`; `scripts/lint_scripts.py` with Ruff 0.16.9 and ShellCheck 0.9.0 | Passed; lint clean, including the new `fuzz` recipe |
| actionlint | Not installed here | Not run on the new `fuzz` job |
| Hosted `Parser fuzzing` job | GitHub Actions, `ubuntu-24.04` | Not run before the commit. Its first run, the manual dispatch [41051151107](https://github.com/chefzaid/bt2usb/actions/runs/41051151107) on that commit later the same day, passed: seed `0x98ed6db03` (the run ID), 20 million mutated inputs per target, 120 million in all in 72 s, no finding; the slowest input took 12.6 ms (`report`) |
| Board/radio/USB acceptance | Physical hardware | Not applicable to the parsers; not performed |

## Validation Record — 2026-10-30, Release SBOM

This record covers the commit that checks off "Release SBOM", split from the
P1 item "Supply-chain and tooling maintenance" in
[TODO.md](../TODO.md#release-provenance-and-supply-chain): `release.py stage`
writes the CycloneDX 1.5 document `bt2usb.cdx.json` with
`scripts/release_sbom.py`, packaging refuses one for another version or
commit, and CI runs the new tests through `-p "release*_test.py"`
([software bill of materials](deployment.md#software-bill-of-materials)). The
checks ran locally on Linux in a container on the change before it was
pushed (the stage and package run on a local commit of it, before the release
notes template gained its SBOM sentence); nothing ran on a board or in a
hosted tag run.

| Check | Environment | Result |
| --- | --- | --- |
| Release helper and SBOM tests | `python3 -m unittest discover -s scripts -p "release*_test.py"`, Python 3 on Linux | Passed: 25 tests, 7 of them new. With every crate's scope forced to `required`, `test_linked_crates_are_required_and_build_time_crates_excluded` fails |
| Script tests and lint | `python3 -m unittest discover -s scripts -p '*_test.py'`; `scripts/lint_scripts.py` with Ruff 0.16.9 and ShellCheck 0.9.0 | Passed, 59 tests; lint clean over 14 Python files |
| Real stage and package | `cargo build --release` with `DEFMT_LOG=info`, then `release.py stage` and `release.py package --tag v0.1.0` with the `GITHUB_*` variables set by hand and `llvm-objcopy` from the pinned toolchain | Passed: the package holds `bt2usb.cdx.json` in `SHA256SUMS`, with 87 linked and 47 build-time components plus the SoftDevice. A second staging wrote byte-identical SBOM bytes; without `GITHUB_REPOSITORY`, staging stopped before creating its output directory; `release.py notes` filled the template with the new SBOM link |
| Crate set against the build's resolver | The SBOM's crates compared with `cargo tree --target thumbv7em-none-eabihf --features embedded` over normal and build edges | Equal. Built from `cargo metadata` alone, it also listed `slab`, which only a test's `futures-util` features pull in; that is why the graph comes from `cargo tree` |
| CycloneDX schema | `JsonStrictValidator` for CycloneDX 1.5 from `cyclonedx-python-lib` in a scratch virtual environment; package URLs parsed with `packageurl-python` | Valid; both Git package URLs parse to a `vcs_url` with the full revision |
| `mask ci` steps | The recipe's script: formatting, Clippy for host, embedded, embedded with `log-sensitive-data`, and simulation, host tests, rustdoc for all five builds, the Markdown checks, release and simulation builds | Passed |
| actionlint | Not installed here | Not run; the workflow change is the quoted unittest pattern in one `run:` line |
| Hosted tag run | GitHub Actions `release-package` | Not performed: no tag was pushed, so attestation of the SBOM is checked only by the existing `subject-path: dist/*` |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-30, Storage And Security Errors On The OLED

This record covers the commit that checks off the P1 item "Visible
storage/security errors" in [TODO.md](../TODO.md#ui-display-and-power): the
**Paired** notices `Replaced old pairing` and `Oldest device removed`, the
errors `Pairing failed`, `Forget, pair again`, `Pair device again`, and
`Kept until power-off`, a background reconnect that stops when its keys are
missing or refused, and the split-out item for unsupported reports. The same
commit corrects the security guide's paragraph on peer Security Requests,
which still described the path the pairing permission closed. The checks ran
locally on Linux in a container on the working tree just before that commit;
nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 431 unit tests (8 new in `coordinator_tests.rs`, `controller_tests.rs`, and `ui_logic_tests.rs`), 3 integration tests, 3 glyph-table tests, 2 pairing-permission tests, and 5 vendored-portal tests. With a replaced bond counted for keys added to a record saved without any, `keys_merge_with_an_entry_stored_under_the_identity_or_a_private_address` fails |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only`, cargo-llvm-cov 0.9.1 | 98.95% of lines, 98.96% of regions, 99.19% of functions |
| `mask ci` steps | The recipe's script: formatting, Clippy for host, embedded, embedded with `log-sensitive-data`, and simulation, host tests, rustdoc for all five builds, the Markdown checks, release and simulation builds | Passed |
| Script tests and lint | `python3 -m unittest discover -s scripts -p '*_test.py'`; `scripts/lint_scripts.py` with Ruff 0.16.9 and ShellCheck 0.9.0 | Passed; lint clean |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` with the platform copy without `ApplySVD` | Passed, all three cases. The simulation has two HID advertisers and pairs nothing, so it reaches none of the new screens; the layout tests cover their text |
| A peer refusing stored keys on air | A peripheral reset or paired elsewhere after bonding | Not performed: that `sd_ble_gap_encrypt` reports a refusal as a security update without encryption is read from the S140 API, whose only documented event for it is `BLE_GAP_EVT_CONN_SEC_UPDATE` |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-30, Pairing Permission

This record covers the commit that fixes the P0 item "Refuse peer-initiated
pairing on background reconnects" in
[TODO.md](../TODO.md#ble-central-and-pairing): the vendored crate's
`SEC_REQUEST` arm now pairs only on a link the application allowed to pair
(`PairingPermission` in `vendor/nrf-softdevice/src/ble/pairing_gate.rs`,
`Connection::allow_pairing`), the slot worker grants that only for a user's
connection, and `Bonder::on_bonded` refuses a bond on any other link
([ADR 0007](adr/0007-vendored-softdevice-patch.md),
[ADR 0011](adr/0011-interim-just-works-pairing.md)). The checks ran locally
on Linux in a container on the working tree just before that commit; nothing
ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 423 unit tests, 3 integration tests, 3 glyph-table tests, 2 new pairing-permission tests, and 5 vendored-portal tests. With `NEW_LINK` set to grant pairing, `a_new_link_ignores_a_security_request_without_keys` fails |
| `mask ci` steps | The recipe's script: formatting, Clippy for host, embedded, embedded with `log-sensitive-data`, and simulation, host tests, rustdoc for all five builds, the Markdown checks, release and simulation builds | Passed |
| Host coverage | Not rerun | The change adds no code to the host library; the new test file is outside the report like `tests/vendor_portal.rs` |
| Headless Renode tests | Not run | The simulation does not run the SoftDevice, so it cannot reach the changed arm |
| The `SEC_REQUEST` arm on air | A peripheral that sends a Security Request after losing its keys | Not performed; the arm is reviewed, not tested |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-30, License Gate, Cleanup, And Storage Versions

This record covers commits `2660200` to `fc9a597`: the CI audit moved from
`cargo audit` to `cargo deny --locked check` with a license allow list and a
source rule, the Renode installer checks its archive's SHA-256, the host
library declares its shared modules inline instead of through `#[path]`
facades, the fixed FIXME entries were shortened, these records moved out of
the testing guide, stale guide statements were corrected, the self-test's
`draw_home` lost two unused parameters, and the storage wire versions and a
capacity-change test were added. The checks ran locally on Linux in a
container on the tree at `fc9a597`, except where a row names another commit;
nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 423 unit tests (1 new in `devices_format_tests.rs`), 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only`, cargo-llvm-cov 0.9.1 | 98.94% of lines, 98.95% of regions, 99.18% of functions |
| `mask ci` steps | The recipe's script: formatting, Clippy for host, embedded, embedded with `log-sensitive-data`, and simulation, host tests, rustdoc for all five builds, the Markdown checks, release and simulation builds | Passed at `9ffd1d9`; formatting, host tests, and the Markdown checks passed again at `fc9a597`, whose only code change is a test |
| Dependency audit | cargo-deny 0.20.2 built with `cargo install --locked`, `cargo deny --locked check` | Passed: advisories, bans, licenses, and sources ok. With the `BSD-2-Clause` allowance removed the license check fails on `Inflector 0.11.4`, and with `RUSTSEC-2026-0110` removed the advisory check fails |
| Renode installer | `scripts/install-renode.sh` with a wrong `RENODE_SHA256`, then the pinned one | The wrong digest stopped the install before extraction and deleted the archive; the pinned digest installed Renode 1.16.1 |
| Script tests and lint | `python3 -m unittest discover -s scripts -p '*_test.py'`; `scripts/lint_scripts.py` with Ruff 0.16.9 and ShellCheck 0.9.0 from the distribution, not the pinned 0.11.0 | Passed: 52 tests; lint clean |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` with the platform copy without `ApplySVD` | Passed at `9ffd1d9` |
| Hosted CI | GitHub Actions | Push run 41034387330 (`2660200`) passed every job, including the new cargo-deny audit and actionlint over the changed workflow; actionlint was not run locally |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Stack Guard Review Fixes

This record covers the commit that fixes the P2 FIXME "The stack guard's
first version missed cases and overstated others" in
[TODO.md](../TODO.md#fixme): the 24 findings a four-part review upheld
against the MPU stack guard commit `ba13354`
([ADR 0026](adr/0026-mpu-stack-guard.md)). `enable_guard` now turns MPU
regions 1 and up off and the self-test's `stack guard` stage checks they stay
off; the HardFault handler also counts a data access the MPU refused inside
the guard as an overflow (`StackGuard::is_overflow`) and drops the PC when
the frame's push hit a bus error (`frame_stacked`); the self-test offers its
deliberate overflow only when the SELECT and guard stages passed, never
takes it from a SELECT already held, and stops the recursion by the guard's
address; the simulation fences its UART buffer before the DMA starts; and
the docs the review named now match the code. The checks ran locally on
Linux in a container, on the working tree just before that commit; nothing
ran on a board. A second, three-part check of that commit found two doc
errors, fixed in a doc-only follow-up: the self-test's next largest frame
is 1,080 bytes, not 1,540 (the scan had merged two functions of the same
name), and the statics are intact only when the reported stack pointer is
at least the fault handler's own stack use above the guard's base, not
anywhere inside the guard.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 422 unit tests (7 new in `stack_logic.rs`, 18 there in all), 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.94% of lines (from 98.92%), 98.95% of regions, 99.18% of functions; `stack_logic.rs` 100% of lines, regions, and functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge `.text` 125,156 bytes with `DEFMT_LOG=debug` and 124,144 with `DEFMT_LOG=info` (+48 each since `a51cec1`); `.bss` 25,380 and `.uninit` 1,048 unchanged, so `_stack_end` stays `0x2000CDA4` and the guard `0x2000D000`–`0x2000E000` |
| Largest frames | `llvm-objdump -d` of the images `mask` builds (release at `DEFMT_LOG=debug`; the simulation's debug build), each function's saved registers plus every prologue `sub sp` immediate, cross-checked against the sum of every `sub sp` in the function, functions named from DWARF | Bridge 2,704 bytes (the display task's poll: 36 saved, 2,668 locals); self-test 4,480 (its main task's poll, whose prologue has two `sub sp`), next largest 1,080 (a `Pin<P> as Future>::poll` instance: 36 saved, 1,044 locals); simulation 5,856 (its main task's poll), next 3,024 (`apply`). The two frames above the 4 KiB guard are main tasks the executor polls only from the top of the stack. `overflow_stack` is 536 bytes (24 saved, 512 locals) and compares its copy's address, the frame's lowest, with the guard's top before recursing. The `ba13354` record's 2,668 and 3,412 counted only one `sub sp` per function, no saved registers, and a `DEFMT_LOG=info` self-test |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 52 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 46 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed: the scenario in 17.54 s, the stack overflow case in 7.62 s (Renode reported `stack pointer 0x20004000`, the guard's top, and `PC not stacked`), and the model checks in 0.63 s, with the local platform copy without `ApplySVD` described in an earlier record |
| Hosted CI | GitHub Actions | Push run 38115627401 (`a51cec1`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed; the regions check, the refused-access rule, and the self-test's new offer conditions and address bound are unrun on a board |

## Validation Record — 2026-10-11, Overflow Report Kept Across A Reset

This record covers the commit that fixes the P2 FIXME "A stack overflow
during a log call is not reported" in [TODO.md](../TODO.md#fixme). The
HardFault handler now stores the overflow as an `OverflowRecord` (new in the
pure `src/stack_logic.rs`) in six atomics in `.uninit` before it logs, and
every image logs and clears it at boot as
`previous boot: stack overflow: ...`, so an overflow whose log call failed,
or that happened with no probe attached, is still reported after a reset
that keeps RAM ([ADR 0026](adr/0026-mpu-stack-guard.md)). The checks ran
locally on Linux in a container, on the working tree just before that
commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 415 unit tests (5 new in `stack_logic.rs`), 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.92% of lines (from 98.90%), 98.92% of regions, 99.16% of functions; `stack_logic.rs` 100% of lines, regions, and functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge `.text` 125,108 bytes with `DEFMT_LOG=debug` and 124,096 with `DEFMT_LOG=info` (+388 each since `ba13354`); `.uninit` 1,048 (+24, the record at `0x2000C98C` before defmt-rtt's buffer, from `llvm-nm -S`); `.rodata`, `.data`, and `.bss` unchanged. `_stack_end` moves to `0x2000CDA4`; the guard stays at `0x2000D000`–`0x2000E000` |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 52 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 46 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed: the scenario in 17.53 s, the stack overflow case, now with a machine reset and the `previous boot` line, in 7.66 s, and the model checks in 0.68 s, with the local platform copy without `ApplySVD` described in an earlier record. With the handler's store removed, the overflow case failed at the `previous boot` line |
| Hosted CI | GitHub Actions | Push run 38114870036 (`ba13354`), the commit before this one, passed every job, including the new Renode overflow case on the stock platform |
| Board/radio/USB acceptance | Physical hardware | Not performed; whether the nRF52840 keeps the record through the reset button and a lock-up rests on the product specification |

## Validation Record — 2026-10-11, MPU Stack Guard

This record covers the commit that completes P1 "Stack overflow detection" in
[TODO.md](../TODO.md#platform-memory-and-recovery)
([ADR 0026](adr/0026-mpu-stack-guard.md)). The new pure
`src/stack_logic.rs` places a 4 KiB guard at the first 4 KiB boundary at or
above `_stack_end` and computes the MPU register values;
`src/stack.rs` programs region 0 with them at boot in all three images,
scans the high-water mark from the guard's top, and adds the HardFault
handler and an assembly MemManage handler that report an overflow. The
self-test checks the MPU after its SoftDevice, USB, and BLE stages and offers
a deliberate overflow on SELECT, and `mask selftest` now passes
`--no-catch-hardfault` to `probe-rs run`, whose default HardFault catch would
halt the core before the handler logs. Writing the docs found one limitation,
listed as an open P2 entry in the [TODO.md FIXME section](../TODO.md#fixme):
an overflow during a `defmt` call loses its report. The checks ran locally on
Linux in a container, on the working tree just before that commit; nothing
ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 410 unit tests (6 new in `stack_logic.rs`), 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.90% of lines (from 98.87%), 98.90% of regions, 99.15% of functions over 33 files; `stack_logic.rs` 100% of lines, regions, and functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge `.text` 124,720 bytes with `DEFMT_LOG=debug` and 123,708 with `DEFMT_LOG=info` (+520 each since `19ba629`); `.rodata` 12,380, `.data` 1,640, `.bss` 25,380, and `.uninit` 1,024 unchanged. `_stack_end` is `0x2000CD8C` (`llvm-nm`), so the guard is `0x2000D000`–`0x2000E000` and 200 KiB of stack remain above it |
| Largest frames | `llvm-objdump -d` of the release images, every `sub sp` immediate form, functions named from DWARF | Bridge 2,668 bytes (the display task's poll), self-test 3,412 (its main task's poll), both under the 4 KiB guard; no frame subtracts a register; `overflow_stack` takes 512 bytes |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 52 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 46 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed: the scenario in 17.47 s, the new stack overflow case in 7.63 s, and the model checks in 0.63 s, with the local platform copy without `ApplySVD` described in an earlier record. With `MPU_CTRL` written as 0, the overflow case failed: no fault was taken and the simulation crashed on the corrupted stack |
| probe-rs HardFault catch | Review of probe-rs-tools 0.32.0 (`cmd/run.rs`, `rpc/utils/run_loop.rs`, `util/cli.rs`) | `probe-rs run` enables the HardFault vector catch unless `--no-catch-hardfault` is given and then exits with `Firmware exited unexpectedly: ...` and a backtrace; not run against a board |
| Hosted CI | GitHub Actions | Push run 38112472769 (`35f1f11`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed; the self-test's `stack guard` stage and deliberate overflow are unrun |

## Validation Record — 2026-10-11, PnP ID Review Fixes

This record covers the commit that fixes the findings an independent review
of the PnP ID commit `2e81820` upheld, listed as a checked P3 entry in the
[TODO.md FIXME section](../TODO.md#fixme). Only documentation and comments
changed: the security guide's input-validation table gained a row for the
Device Information discovery and PnP ID read, the architecture layer table
and dependency graph gained `ble::pnp_id`, `ble::device_info`, and
`diagnostics`, and every other flagged statement now matches the code. The
checks ran locally on Linux in a container, on the working tree just before
that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 404 unit tests, 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests; no test changed |
| Clippy with warnings denied | Host tests, embedded | Passed; the code changes are comments only |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, `bt2usb` | Passed |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Hosted CI | GitHub Actions | Push run 38111305224 (`19ba629`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Counter Review Fixes

This record covers the commit that fixes the findings an independent review
of the event-counter commit `78471e8` upheld, listed as a checked P2 entry in
the [TODO.md FIXME section](../TODO.md#fixme). `run_endpoint`
(`src/hid/delivery.rs`) now resets its backoff, and with it the test for a
first failure, when a USB lifecycle event ends the backoff wait or the worker
finds USB unavailable, so `USB write failures` and the endpoint warning count
the first failure after every bus reset, resume, or reconfiguration as the
docs say. The new host test
`only_the_first_failure_since_a_write_or_bus_change_is_reported` failed on
the old worker (1 first failure where 2 were due) and passes on the new one.
The first-flash Counters check now starts from a clean boot and expects one
lost link and one reconnect attempt per sleep; the `reports coalesced`
docs give the real causes and what a merge loses; and the security row, the
flash-retries row, the test map, ADR 0003, and the feature guide match the
code. The checks ran locally on Linux in a container, on the working tree
just before that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 404 unit tests (1 new in `hid/delivery_tests.rs`), 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.87% of lines (from 98.81%), 98.86% of regions, 99.12% of functions; `hid/delivery.rs` 99.48% of lines and every function |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge `.text` 124,200 bytes with `DEFMT_LOG=debug` and 123,188 with `DEFMT_LOG=info` (+16 each since `2e81820`, the worker's two extra resets); `.rodata`, `.data`, and `.bss` unchanged |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 52 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Hosted CI | GitHub Actions | Push run 38110418581 (`2e81820`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed; the reworked first-flash Counters check is unrun |

## Validation Record — 2026-10-11, Peripheral PnP ID

This record covers the commit that completes P1 "Diagnostics without
sensitive input" in [TODO.md](../TODO.md#platform-memory-and-recovery). The
new pure `src/ble/pnp_id.rs` parses a Device Information Service PnP ID, and
the new shell `src/ble/device_info.rs` discovers the service and reads and
logs the value once per link. `run_notification_loop` runs it in the future
that writes host LED state, before the first write, because the vendored GATT
client allows one client procedure per link while notifications use their
own portal. The runbook's [PnP ID](operations.md#peripheral-pnp-id) section,
the log reference, the first-flash pairing checks, and both issue templates
describe or ask for the line. The ADR 0003 implementation list and the host
library composition row, which lacked `bond_table.rs`, `scan_list.rs`, and
`messages.rs`, now list every pure BLE module. The checks ran locally on
Linux in a container, on the working tree just before that commit; nothing
ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 403 unit tests (3 new in `ble/pnp_id.rs`), 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.81% of lines (from 98.80%), 98.80% of regions, 99.12% of functions; `ble/pnp_id.rs` 100% of lines, regions, and functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A`: with `DEFMT_LOG=debug`, `.text` 124,184 bytes (+3,128 since `78471e8`, most of it a second instance of the vendored generic `gatt_client::discover`), `.rodata` 12,380 (+56), `.data` 1,640, `.bss` 25,380 (+656: `ble_slot_task::POOL` grows from 5,760 to 6,416 bytes for the two workers, since the discovery and read futures sit beside the notification and drain futures, measured with `llvm-nm -S`), `.uninit` 1,024; with `DEFMT_LOG=info`, `.text` 123,172 bytes |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 52 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed, with the local platform copy without `ApplySVD` described in an earlier record; the simulation has no GATT client, so it does not read a PnP ID |
| PnP ID on the target | Review | Built and reviewed, not tested: no host test or Renode scenario runs the GATT discovery and read; the first-flash [pairing and daily use](first-flash.md#4-pairing-and-daily-use) check records each peripheral's line |
| Hosted CI | GitHub Actions | Push run 38110033129 (`ccd4a1c`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Bond And Portal Doc Corrections

This record covers the commit that closes the FIXME "Docs the bond-table and
portal commits left wrong" in [TODO.md](../TODO.md#fixme). It rewrites the
first-flash "Unsaved pairing evicts nothing" and "Fifth device" checks so a
tester can run them and expects what the code does, and corrects
[features](features.md#connection-and-security), ADR 0017, ADR 0007, the
vendored README, the `Bond table full` row in
[operations](operations.md#ble-scan-and-connection), the
`DiscardUnsavedBond` wording in the data model, architecture, security, and
two coordinator comments, and the coverage row of the
[Bond Identity Refusal record](#validation-record--2026-10-10-bond-identity-refusal),
which a rerun of that commit's tree measured at 98.89% of lines and 98.88%
of regions. No code changed. The checks ran locally on Linux in a container,
on the working tree just before that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit tests | `cargo test --locked --lib` | Passed; only comments changed in code |
| Rustdoc with private items, warnings denied | Host library | Passed, with the reworded intra-doc links in `coordinator_events.rs` |
| Documentation checker | `python3 scripts/check_docs.py` | Passed |
| Scan listing for the new check | Review of `merge_advertisement` in [scan_list.rs](../src/ble/scan_list.rs) | A device enters the list only when its advertisement carries `0x1812`; the check now makes the phone advertise it. That a given phone app can advertise the UUID and accept bonding is not verified here |
| Hosted CI | GitHub Actions | Push run 38109817253 (`78471e8`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Event Counters

This record covers the commit that adds the bounded counters of P1
"Diagnostics without sensitive input" in
[TODO.md](../TODO.md#platform-memory-and-recovery). The pure
`src/diagnostics.rs` gains nine saturating `AtomicU32` counters
(`COUNTERS`) and `CounterReport`, which decides when the UI loop's 1-second
tick logs `diagnostics: {}`: at the first change after a quiet spell, then at
most once per `DIAGNOSTICS_REPORT_INTERVAL_SECS` (60 s), never while the counts
stay the same. `ReportCoalescer::push` and `EndpointDelivery::publish` now
return whether they coalesced or collapsed a full queue; the slot worker, the
notification loop and LED writer, the endpoint mailboxes, and the store shell
bump the counters. The [event counters](operations.md#event-counters) section
documents each count and how to collect it. The checks ran locally on Linux
in a container, on the working tree just before that commit; nothing ran on a
board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 400 unit tests (7 new in `diagnostics_tests.rs`, 1 in `hid/coalesce.rs`, 1 in `hid/delivery.rs`), 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.80% of lines, 98.79% of regions, 99.11% of functions (from 99.08%); `diagnostics.rs` 98.86% of lines: the uncovered line is the out-of-range branch of `Counters::bump`, which a `Counter` cannot reach |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five, after replacing two intra-doc links to `hid` items, which the simulation build does not contain, with plain code spans |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A`: with `DEFMT_LOG=debug`, `.text` 121,056 bytes (+1,132 since `a621ef2`), `.rodata` 12,324 (+240: the counter names), `.data` 1,640, `.bss` 24,724 (+88: `COUNTERS` takes 36 bytes and `__embassy_main::POOL` grows from 712 to 760 for the `CounterReport`, measured with `llvm-nm -S`; `ble_slot_task::POOL` stays at 5,760), `.uninit` 1,024; with `DEFMT_LOG=info`, `.text` 120,072 bytes |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 52 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files, including the new `DIAGNOSTICS_REPORT_INTERVAL_SECS` row in the [configuration defaults](hardware.md#configuration-defaults) |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed, with the local platform copy without `ApplySVD` described in an earlier record; the simulation build does not run the shells that count, so it logs no `diagnostics:` line |
| Counters on the target | Review | Built and reviewed, not tested: no host test or Renode scenario runs the slot worker, the USB mailboxes, or the store shell that bump the counters; the first-flash [pairing and daily use](first-flash.md#4-pairing-and-daily-use) checks now record the latest line |
| Hosted CI | GitHub Actions | Push run 38109571606 (`8425cb6`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Portal Guard Comments

This record covers the commit that closes the FIXME "The portal guard's
comments describe the old behaviour" in [TODO.md](../TODO.md#fixme). It
changes only the two comments above the drop guard in the vendored
`vendor/nrf-softdevice/src/util/portal.rs`, which now state that the guard
clears the portal only while it holds the wait's own closure, as the
[portal fix](#validation-record--2026-10-11-portal-clears-only-its-own-wait)
made it do. No code changed. The checks ran locally on Linux in a container,
on the working tree just before that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Vendored-portal tests | `cargo test --locked --test vendor_portal` | Passed: 5 tests, compiled from the edited file |
| Documentation checker | `python3 scripts/check_docs.py` | Passed |
| Hosted CI | GitHub Actions | Push run 38108965919 (`a621ef2`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Boot Identity And Reset Reason

This record covers the commit that starts P1 "Diagnostics without sensitive
input" in [TODO.md](../TODO.md#platform-memory-and-recovery). `build.rs`
passes the git commit (with `-dirty` for a modified checkout), the Cargo
profile, and the `DEFMT_LOG` filter to the new pure `src/diagnostics.rs`; the
bridge, the self-test, and the simulation log them at boot, and the bridge
reads, clears, and logs the decoded `POWER.RESETREAS` causes before the
SoftDevice starts. `scripts/release.py stage` now refuses an application or
self-test image that does not report the checked-out commit clean. The checks
ran locally on Linux in a container, on the working tree just before that
commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 391 unit tests (7 new in `diagnostics_tests.rs`), 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.80% of lines (from 98.79%), 98.79% of regions, 99.08% of functions; `diagnostics.rs` 100% of lines |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A`: with `DEFMT_LOG=debug`, `.text` 119,924 bytes (+420 since `55b6492`), `.rodata` 12,084 (+424: the identity strings and the reset cause names), `.data` 1,640, `.bss` 24,636 (+8: `__embassy_main::POOL` grows from 704 to 712 bytes, measured with `llvm-nm -S`), `.uninit` 1,024; with `DEFMT_LOG=info`, `.text` 118,944 bytes |
| Embedded identity | `strings` on the release `bt2usb` ELF | The version `0.1.0`, the working tree's commit followed by `-dirty` (the tree held the uncommitted change), and the profile strings are in `.rodata` |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 52 tests, 2 skipped, including the new staging refusal test; `ruff check` and `ruff format --check` passed |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed, with the local platform copy without `ApplySVD` described in an earlier record; the new pattern matched `version 0.1.0, commit <working-tree commit>-dirty, debug build` |
| Reset reason on the target | Review | Built and reviewed, not tested: Renode's nRF52840 model and the host tests do not run `main`'s register read; which causes a board reports after `mask run`, the reset button, and a power cycle is a first-flash check ([3. Real firmware](first-flash.md#3-real-firmware)) |
| Hosted CI | GitHub Actions | Push run 38108019218 (`55b6492`), the commit before this one, passed every job, including the vendored-portal tests on Windows |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Portal Clears Only Its Own Wait

This record covers the commit that fixes what the independent review of
"Drop a superseded link's GATT waits before closing it" upheld (FIXME "A GATT
wait that ends after a peer disconnect can clear the next link's portal" in
[TODO.md](../TODO.md#fixme)). A `bt2usb patch:` in the vendored
`util/portal.rs` makes the drop guard of `Portal::wait_once` and `wait_many`
clear the portal only while it still holds that wait's own closure, and the
new `tests/vendor_portal.rs` compiles the vendored portal on the host. The
review's other findings, two TODO sentences and the coverage rows that gave
the region figure as the line figure, were corrected in the commit before
this one. The checks ran locally on Linux in a container, on the working tree
just before that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 384 unit tests, 3 integration tests, 3 glyph-table tests, and 5 vendored-portal tests |
| Portal tests against the upstream portal | `tests/vendor_portal.rs` with `util/portal.rs` reset to the commit before | 3 of 5 failed, as expected: the three that register a wait after an earlier one ended; the cancellation and keep-waiting tests passed |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.79% of lines, 98.79% of regions, 99.06% of functions, unchanged: neither the test file nor the vendored portal it compiles is in the report |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed; `tests/vendor_portal.rs` is a new root, and its modules into `vendor/` are skipped |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A`: with `DEFMT_LOG=debug`, `.text` 119,504 bytes (+188 since `b6fdc8f`), `.rodata` 11,660 (−16), `.data` 1,640, `.bss` 24,628 (+32: `ble_slot_task::POOL` grows from 5,728 to 5,760 bytes, measured with `llvm-nm -S`, because each pending wait's guard now keeps its closure's address), `.uninit` 1,024; with `DEFMT_LOG=info`, `.text` 118,524 bytes |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 51 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed, with the local platform copy without `ApplySVD` described in an earlier record; the simulation does not compile the vendored crate |
| Firmware portal behaviour | Review | Built and reviewed, not tested on the target: the host tests run the vendored source with embassy-sync's host thread-mode mutex, while the firmware runs it on the SoftDevice event path; the event order that triggered the hang (a peer's `DISCONNECTED` and the other slot's `CONNECTED` on the freed handle in one event drain) was read from the code, not reproduced |
| Hosted CI | GitHub Actions | Push run 38107545021 (`b6fdc8f`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Bonds Follow The Saved Devices

This record covers the commit that keeps the keys `Bonder` holds in RAM in
step with the device store (FIXME "Keys held in RAM and saved records are
evicted separately" in [TODO.md](../TODO.md#fixme)). The new pure
`BondTable` holds the saved devices' keys plus one unsaved pairing per link;
a new pairing is added unsaved instead of dropping the oldest bond;
`PersistDevice` marks the keys saved and drops the keys of a record the store
evicts; and an attempt that ends before its link was reported up returns
`Action::DiscardUnsavedBond`, which drops its pairing's unsaved keys. The
slot-event reducers moved to `coordinator_events.rs`, a child module of
`coordinator.rs`. The checks ran locally on Linux in a container, on the
working tree just before that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 384 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.79% of lines (from 98.91%), 98.79% of regions, 99.06% of functions; `ble/coordinator_events.rs` 100% of lines. Every line of `ble/bond_table.rs` runs in the line report, but the summary gives it 91.80% of lines because it counts each closure's instantiation of the generic methods on its own, and no single test drives every branch with the same closure |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A`: with `DEFMT_LOG=debug`, `.text` 119,316 bytes (+1,840 since `44fa5c7`: the table's methods, instantiated for each caller's closure, and the save, evict and discard actions), `.rodata` 11,676 (+48, the new log lines), `.data` 1,640, `.bss` 24,596 (+112: `Bonder`'s table holds six bonds instead of four, 348 bytes in all; `ble_slot_task::POOL` is still 5,728 bytes), `.uninit` 1,024; with `DEFMT_LOG=info`, `.text` 118,300 bytes |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 51 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed, with the local platform copy without `ApplySVD` described in an earlier record |
| Bonder and store wiring | Review | Built and reviewed, not tested: `Bonder`, `execute_action`, and the `DeviceStore` shell depend on SoftDevice types. The table's policy, the store's eviction outcome, and the reducers' discard action are host-tested |
| Hosted CI | GitHub Actions | Push run 38106172492 (`44fa5c7`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed; the "Fifth device" and "Unsaved pairing evicts nothing" checks in [6. Device management and degraded display](first-flash.md#6-device-management-and-degraded-display) exercise saving, evicting and discarding on a board |

## Validation Record — 2026-10-11, Link Dropped During Discovery

This record covers the commit that retries a background reconnect whose link
drops while its HID service is discovered (FIXME "A background reconnect that
loses its link during HID discovery stops retrying" in
[TODO.md](../TODO.md#fixme)). When securing or discovering fails,
`connect_and_run_secure` reads the link's handle, and
`ErrorTag::for_failed_setup` reports `ConnectFailed` for a link that has gone,
which a silent attempt retries; a failure on a link still up keeps its own
error. One attempt's connect, secure, discover, and run phases moved from
`slot_worker.rs` to the new `slot_link.rs` to keep both under 500 lines. The
checks ran locally on Linux in a container, on the working tree just before
that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 375 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.91% of lines, 98.90% of regions, 99.02% of functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A`: with `DEFMT_LOG=debug`, `.text` 117,476 bytes (+56 since `5e9403a`), `.rodata` 11,628 (+20, the new log line), `.data` 1,640, `.bss` 24,484 (unchanged; `ble_slot_task::POOL` is still 5,728 bytes), `.uninit` 1,024; with `DEFMT_LOG=info`, `.text` 116,456 bytes |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 51 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed, with the local platform copy without `ApplySVD` described in an earlier record |
| Link-drop detection in the slot | Review | Built and reviewed, not tested: the order in which the vendored crate clears the handle and fails the GATT procedure (`on_disconnected` in `vendor/nrf-softdevice/src/ble/connection.rs` sets the handle state to disconnected before it calls the GATT portals) was read from the code. A peer that fails discovery on a link that stays up and disconnects at once can be taken for a dropped link and retried |
| Hosted CI | GitHub Actions | Push run 38105739061 (`5e9403a`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Retry Takeover Review Fixes

This record covers the commit that fixes what the independent review of the
[retry-takeover commit](#validation-record--2026-10-11-retry-takeover-races)
upheld (FIXME "A superseded link's pending GATT wait could outlive it" in
[TODO.md](../TODO.md#fixme)). The slot worker drops its pinned security,
discovery, and notification futures before it closes a superseded link; one
host test covers the release of a reserved slot without a link; and the
documents now describe the RAM and store evictions separately, the Security
Request window over the whole life of a background-opened link, and the boot
selection of bonded records only. Keeping the two evictions in step is the
open FIXME "Keys held in RAM and saved records are evicted separately". The
checks ran locally on Linux in a container, on the working tree just before
that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 374 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.91% of lines, 98.90% of regions, 99.01% of functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A`: with `DEFMT_LOG=debug`, `.text` 117,420 bytes (−88 since `b13a1bf`), `.rodata` 11,608, `.data` 1,640, `.bss` 24,484 (−576: with each future pinned in its own block, the two slot-worker task futures in `ble_slot_task::POOL` shrink from 3,152 to 2,864 bytes each, measured with `llvm-nm -S`), `.uninit` 1,024; with `DEFMT_LOG=info`, `.text` 116,400 bytes |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 51 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed, with the local platform copy without `ApplySVD` described in an earlier record |
| Slot worker drop order | Review | Built and reviewed, not tested: the GATT portals and the reuse of a connection handle exist only in the SoftDevice, and the window needs the other slot to connect within the 10 ms the closing worker polls |
| Hosted CI | GitHub Actions | Push run 38102873180 (`b13a1bf`), the commit before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed |

## Validation Record — 2026-10-11, Retry Takeover Races

This record covers the commit that numbers each slot attempt, keeps a link or
attempt already under way when a selection takes over a background retry,
hands the slot back to the retry when that connection fails while the
device's keys remain, and runs a background retry only while `Bonder` holds
the device's keys (FIXME "The retry takeover raced the slot's own events" in
[TODO.md](../TODO.md#fixme)). The checks ran locally on Linux in a container,
on the working tree just before that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 373 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.91% of lines, 98.90% of regions, 99.01% of functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A`: with `DEFMT_LOG=debug`, `.text` 117,508 bytes (+1,112 since `01200b1`), `.rodata` 11,608, `.data` 1,640, `.bss` 25,060 (+1,784, measured with `llvm-nm -S`: 1,712 is the two slot-worker task futures in `ble_slot_task::POOL` growing from 2,296 to 3,152 bytes each, 24 is the `ble_task` future, and 48 is spread over smaller statics such as the larger slot command and event channels), `.uninit` 1,024; with `DEFMT_LOG=info`, `.text` 116,552 bytes |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 51 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed, with the local platform copy without `ApplySVD` described in an earlier record |
| Slot worker and boot filter | Review | Built and reviewed, not tested: they depend on SoftDevice types. The coordinator reducers the takeover relies on are host-tested in `coordinator_attempt_tests.rs`; the review found the release of a reserved slot without a link untested, and the follow-up added that test |
| Independent review | Reviewers told to refute the change (protocol, security, documentation, tests), each finding then checked by a second reviewer told to refute it | Finished after this commit: 19 findings upheld, two of them duplicates, and 3 refuted. The upheld ones (a GATT wait that could outlive its link, the untested slot release, the RAM and store evictions described as one, and stale documents) were fixed in the next commit ([review fixes](#validation-record--2026-10-11-retry-takeover-review-fixes)) |
| Hosted CI | GitHub Actions | Push runs 38098634874 (`8474673`) and 38099057792 (`01200b1`), the commits before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed; the change needs the "Select a reconnecting device" check in the first-flash checklist ([4. Pairing and daily use](first-flash.md#4-pairing-and-daily-use)) |

## Validation Record — 2026-10-11, Refused Pairing Not Stored

This record covers the commit that refuses a pairing whose identity is not a
public or random static address in `Bonder::on_bonded` and stores nothing for
the device, instead of storing it without keys (FIXME "A refused pairing
still stored the device" in [TODO.md](../TODO.md#fixme)). The checks ran
locally on Linux in a container, on the working tree just before that commit;
nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 366 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.89% of lines, 98.88% of regions, 99.00% of functions |
| Mutation check | Host tests | With `DeviceList::add` storing the device without keys instead of refusing it, all three refusal tests fail |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A`: with `DEFMT_LOG=debug`, `.text` 115,372 bytes (+824 since `405111c`), `.rodata` 11,576, `.data` 1,640, `.bss` 23,276, `.uninit` 1,024; with `DEFMT_LOG=info`, `.text` 114,220 bytes |
| Script tests | `python3 -m unittest discover -s scripts -p '*_test.py'` | Passed: 51 tests, 2 skipped |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed, with the local platform copy without `ApplySVD` described in the previous record |
| `Bonder` and `execute_action` | Review | Built and reviewed, not tested: they depend on SoftDevice types, and no peer that sends such an identity was available |
| Independent review | Reviewers told to refute the change (code, documentation), each finding then checked by a second reviewer told to refute it | Code: three findings, none upheld. Bonding can finish after the slot reports `Connected`, but that is older than this change and gives a peer nothing it cannot get by not bonding; the refused list can lose a pending entry only under overlapping, deliberately stalled pairings; and an unbonded peer is still stored without keys, which predates the change and which the proposed pairing policy refuses ([ADR 0017](adr/0017-authenticated-pairing-and-enrollment.md)). Documentation: the upheld findings were fixed before the commit: the `Pairing not saved` row and step 5 in features.md, the `ErrorTag::BondRefused` rustdoc, the `AddOutcome` log list and refusal order in data-model.md, the no-identity case in operations.md and data-model.md, the tense of the P1 entry and of the open FIXME titles, and a link to "Visible storage/security errors". Test counts left stale by the panic-lint commit were corrected at the same time |
| Hosted CI | GitHub Actions | Push runs 38094862268 (`b71bca8`), 38095323044 (`e063228`), 38095540735 (`db21887`), and 38095843555 (`405111c`), the commits before this one, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed; the change needs the pairing checks in the first-flash checklist ([4. Pairing and daily use](first-flash.md#4-pairing-and-daily-use)) |

## Validation Record — 2026-10-10, Bond Identity Refusal

This record covers the commit that keeps the pairing store from saving a bond
whose identity is not a public or random static address, decodes the
identity's address type without the vendored `Address::address_type`, and
shows `Pairing not saved` when it refuses one (FIXME "A bond with a private or
reserved identity address broke the store" in [TODO.md](../TODO.md#fixme)).
The checks ran locally on Linux in a container, on the working tree just
before that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 364 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.89% of lines, 98.88% of regions, 99.00% of functions; `storage/devices.rs` 100% of lines, 99.66% of regions |
| Mutation checks | Host tests | Without the identity filter in `DeviceList::add`, two of the new tests fail; without it in `codec::decode_bond`, the per-type format test fails |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation, each after `cargo clean -p bt2usb` | Passed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five ([commands](code-quality.md#documentation-comments)) |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A` on the release ELF: with `DEFMT_LOG=debug` (the `.cargo/config.toml` default), `.text` 113,748 bytes (+192), `.rodata` 11,512 (+24), `.data` 1,640, `.bss` 23,252, `.uninit` 1,024; with `DEFMT_LOG=info`, the release setting, `.text` 112,700 bytes |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 17 tests |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed. The run used a local copy of `platforms/cpus/nrf52840.repl` without its `ApplySVD` line, because this container's proxy blocks the SVD download; hosted CI uses the stock platform |
| Dependency audit, actionlint | — | Not run locally; `Cargo.lock` and the workflows did not change |
| Storage shell and `execute_action` | Review | Reviewed and built, not tested: they depend on SoftDevice types, and no peer that sends such an identity was available |
| Independent review | Reviewers told to refute the change (code, documentation, behavior) | Found that dropping the refused keys with `Bonder::forget`, which matches by address, also dropped every other bond made without an IRK (the all-zero IRK resolves an address built from it), that the documents said a refused device pairs again by itself although background reconnects never pair, and stale test counts and panic notes. The follow-up commit drops exactly the refused bond (`Bonder::forget_bond`) and corrects the documents; the all-zero IRK became a FIXME |
| Hosted CI | GitHub Actions | Push runs 38093412238 (`54a6d28`), 38093509957 (`b2cb34d`), and 38093693079 (`50ada0a`), for the three MTU fixes before this commit, passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed; the change needs the pairing checks in the first-flash checklist ([4. Pairing and daily use](first-flash.md#4-pairing-and-daily-use)) |

## Validation Record — 2026-10-10, Panic Lints And Inventory

This record covers the commit that turns on Clippy's panic lints, rewrites the
flagged sites, checks the flash range and the BLE event buffer at compile
time, removes four vendored panics a peer could reach, and lists the remaining
panic paths ([ADR 0025](adr/0025-panic-lints-and-inventory.md)). The checks
ran locally on Linux in a container, on the working tree just before that
commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 361 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.88% of lines (from 98.16%), 98.79% of regions, 98.99% of functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation, each after `cargo clean -p bt2usb` | Passed. An added `.unwrap()` in `LongRead::offset` failed the host run with `unwrap_used`, so the lints are live |
| Compile-time checks | Embedded build with a changed constant | A 131-byte event buffer in `sd_setup.rs` fails the build and 132 bytes passes, so the computed worst case at ATT MTU 64 is 132 bytes; a misaligned flash range in `selftest.rs` fails the build |
| Mutation checks | Host tests | The two new descriptor tests fail when the size checks they cover are removed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five ([commands](code-quality.md#documentation-comments)) |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A` on the release ELF: with `DEFMT_LOG=debug` (the `.cargo/config.toml` default), `.text` 112,912 bytes (+64), `.rodata` 11,488 (−220), `.data` 1,640, `.bss` 23,252 (+16), `.uninit` 1,024; with `DEFMT_LOG=info`, the release setting, `.text` 112,180 bytes and the other sections unchanged. The 128 extra bytes of the event buffer are on `softdevice_task`'s stack, not in `.bss` |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 17 tests |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed: the scenario in 18.00 s and the model checks in 1.30 s. The run used a local copy of `platforms/cpus/nrf52840.repl` without its `ApplySVD` line, because this container's proxy blocks the SVD download; hosted CI uses the stock platform |
| Dependency audit, actionlint | — | Not run locally; `Cargo.lock` and the workflows did not change (enabling a feature of an existing dependency does not change the lock file) |
| Vendored panic fixes | Review | Reviewed, not tested: no test reaches the vendored crate, and exercising the fixed paths needs a peer that misbehaves on purpose |
| Hosted CI | GitHub Actions | Push run 38093287153 for `5783f5b` passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed; the change needs the pairing, reconnect, and device-management checks in the first-flash checklist ([4. Pairing and daily use](first-flash.md#4-pairing-and-daily-use)) |

## Validation Record — 2026-10-10, OLED In Renode

This record covers the commit that moves each screen's text into
`ui::layout`, shares the TWIM0 setup and the display task between the bridge,
the self-test, and the simulation, clears the panel's RAM before the
`ssd1306` driver turns it on, and runs the display task in Renode on the TWIM
and SSD1306 models of [ADR 0024](adr/0024-renode-oled-models.md). The checks
ran locally on Linux in a container, on the working tree just before that
commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 353 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.16% of lines, 98.53% of regions, 99.20% of functions; `ui/layout.rs` 100% of lines |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --all` | No changes left |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five ([commands](code-quality.md#documentation-comments)) |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A` on the release ELF: with `DEFMT_LOG=debug` (the `.cargo/config.toml` default), `.text` 112,848 bytes, `.rodata` 11,708, `.data` 1,640, `.bss` 23,236, `.uninit` 1,024; with `DEFMT_LOG=info`, the release setting, `.text` 111,868 bytes and the other sections unchanged |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 17 tests |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 44 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed: the scenario in 18.49 s and the model checks in 1.36 s. With the RAM clear removed from `display::initialize`, the scenario failed at boot with 2 noisy bytes. The run used a local copy of `platforms/cpus/nrf52840.repl` without its `ApplySVD` line, because this container's proxy blocks the SVD download; hosted CI uses the stock platform |
| Dependency audit, actionlint | — | Not run locally; `Cargo.lock` and the workflows did not change (`embedded-graphics`, already a dependency, is added as a dev-dependency) |
| Hosted CI | GitHub Actions | Push run 38085444796 for `bbe5a83` failed one job: the Windows host tests read the glyph table with CRLF line endings (FIXME "The glyph-table test failed on Windows" in [TODO.md](../TODO.md#fixme)); the fix in `ed63657` passed every job of push run 38085612272, including the Windows host tests and the Renode test on the stock platform |
| Board/radio/USB acceptance | Physical hardware | Not performed; the display change needs the OLED checks in the first-flash checklist, including a power-up without noise ([6. Device management and degraded display](first-flash.md#6-device-management-and-degraded-display)) |

## Validation Record — 2026-10-10, UI Controller And Renode Scenario

This record covers the commit that moves the UI loop's decisions into
`ui::controller` with the shared `ble::messages` types, removes the unused
redraw hint from the button reducer, and broadens the Renode scenario to link
loss and saved-device management (list, cancelled forget, forget, factory
reset) through [`src/sim_ble.rs`](../src/sim_ble.rs). The checks ran locally on
Linux in a container, on the working tree just before that commit; nothing ran
on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 341 unit tests and 3 integration tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.09% of lines, 98.47% of regions, 99.19% of functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting and host API documentation | `cargo fmt --package bt2usb -- --check`; rustdoc with warnings denied | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five ([commands](code-quality.md#documentation-comments)) |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A` on the release ELF: with `DEFMT_LOG=debug` (the `.cargo/config.toml` default), `.text` 111,648 bytes, `.rodata` 11,796, `.data` 1,640, `.bss` 23,204, `.uninit` 1,024; with `DEFMT_LOG=info`, the release setting, `.text` 110,676 bytes and the other sections unchanged |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 17 tests |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 43 Markdown files |
| Headless Renode scenario | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed: 1 test in 18.55 s. The run used a local copy of `platforms/cpus/nrf52840.repl` without its `ApplySVD` line, because this container's proxy blocks the SVD download; hosted CI uses the stock platform |
| Dependency audit, actionlint | — | Not run locally; `Cargo.lock` and the workflows did not change |
| Hosted CI | GitHub Actions | Push run 38081477998 for `61e1963` passed every job, including the Renode simulation test with the stock platform |
| Board/radio/USB acceptance | Physical hardware | Not performed; the UI loop change needs the pairing and device-management checks in the first-flash checklist ([4. Pairing and daily use](first-flash.md#4-pairing-and-daily-use), [6. Device management and degraded display](first-flash.md#6-device-management-and-degraded-display)) |

## Validation Record — 2026-10-10

This record covers the FIXME fixes of 2026-10-10, from "Decide the keyboard
report in one place" (`e1447c7`) through "Set the sighting lifetime in
config.rs" (`6e1b8b4`): the keyboard-only Report Map rule in one place, the
connectable-only reconnect scan, the reconnect wakes and saved-device identity
in the pure table, the simplified connection-parameter helpers, the sighting
lifetime in `config.rs`, and the tests and documentation fixes listed under
[FIXME](../TODO.md#fixme). The checks ran locally on Linux in a container, on
the tree at `6e1b8b4`; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 291 unit tests and 3 integration tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 97.59% of lines, 98.17% of regions, 98.67% of functions |
| Clippy with warnings denied | Host tests, embedded, simulation | Passed |
| Formatting and host API documentation | `cargo fmt --package bt2usb -- --check`; rustdoc with warnings denied | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five ([commands](code-quality.md#documentation-comments)) |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A` on the release ELF: with `DEFMT_LOG=debug` (the `.cargo/config.toml` default), `.text` 110,812 bytes, `.rodata` 11,688, `.data` 1,624, `.bss` 23,300, `.uninit` 1,024; with `DEFMT_LOG=info`, the release setting, `.text` 109,584 bytes and the other sections unchanged |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 12 tests |
| Local Markdown links and anchors | Script over every tracked `.md` file | Passed: 1,968 links in 35 files |
| Headless Renode scenario | — | Not run locally: Renode is not installed in this environment; the scenario does not include the SoftDevice, so it would not exercise the reconnect changes |
| Dependency audit, actionlint | — | Not run locally; `Cargo.lock` and the workflows did not change |
| Hosted CI | GitHub Actions | Passed on `6e1b8b4` ([run 38064576663](https://github.com/chefzaid/bt2usb/actions/runs/38064576663)) and on every earlier commit in the range that was not superseded by a newer push |
| Board/radio/USB acceptance | Physical hardware | Not performed; the reconnect handover, the connectable-only scan, and the LED start state need the board checks in the [first-flash checklist](first-flash.md) |

## Validation Record — 2026-10-09

This record covers the commit that adds the shared reconnect scan
([ADR 0015](adr/0015-shared-reconnect-scan.md)), host-LED forwarding on every
new keyboard link, bounded peripheral connection parameters
([ADR 0016](adr/0016-bounded-peer-connection-parameters.md)), and the reserved
keyboard byte handling. The checks ran locally on Linux in a container, on the
working tree just before that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 260 unit tests and 3 integration tests |
| Clippy with warnings denied | Host tests, embedded, simulation | Passed |
| Formatting and host API documentation | `cargo fmt --package bt2usb -- --check`; rustdoc with warnings denied | Passed |
| Release bridge and self-test builds | Rust 1.95.0, ARM target | Passed; `.text` 111,616 bytes, `.bss` 23,476 bytes, read from the release ELF's section headers, with the default `DEFMT_LOG=debug` (the level was not recorded at the time; a rebuild of that commit on 2026-10-10 reproduced these figures at `debug` and gave `.text` 110,400 bytes at `info`) |
| Simulation build | Rust 1.95.0, ARM target | Passed |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 12 tests |
| Local Markdown links and anchors | Script over every tracked `.md` file | Passed |
| Headless Renode scenario | — | Not run: Renode is not installed in this environment; the scenario does not include the SoftDevice, so it would not exercise these changes |
| Dependency audit, actionlint | — | Not run: neither tool is installed in this environment; `Cargo.lock` and the workflows did not change |
| Hosted CI | GitHub Actions | Runs on the push; not recorded here |
| Board/radio/USB acceptance | Physical hardware | Not performed; the changes need the board checks in the [first-flash checklist](first-flash.md) |

## Validation Record — 2026-09-28

This record covers the current uncommitted hardening changes, not a published
release or a completed hardware qualification. Associate the final commit and
artifact hash with this record when preparing a release.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Windows | Passed: 219 unit tests and 3 integration tests |
| Headless Renode scenario | Renode 1.16.1 through WSL | Passed: 1 scenario |
| Dependency audit | Current locked dependency graph | No vulnerability errors; 2 unmaintained dependency warnings below |
| Release helper policy/integrity tests | Windows Python 3.14 and WSL Python 3.12 | Passed: 12 tests on each platform |
| Workflow syntax and expressions | actionlint 1.7.12 (checksum-verified) with ShellCheck 0.11.0 | Passed locally; the published-release guard's `gh --jq` filter was not executed locally |
| Clippy with warnings denied | Host tests, embedded, simulation | Passed |
| Release bridge and self-test builds | Rust 1.95.0, ARM target | Passed |
| Simulation build | Rust 1.95.0, ARM target | Passed |
| Formatting and host API documentation | Rustfmt; rustdoc with warnings denied | Passed |
| Invalid combined feature selection | Cargo check with `--keep-going` | Rejected by the explicit firmware/simulation build guard |
| Tooling and documentation | Workflow YAML, action pins, shell scripts, 31 mask recipes, local links | Validated; every action SHA resolves to its commented upstream tag (`git ls-remote`); WSL-to-Windows Cargo fallback smoke check passed |
| GitHub Actions workflow | Remote CI | Not run in this session; later hosted runs passed (see below) |
| Board/radio/USB acceptance | Physical hardware | Not performed in this validation session |

The dependency audit reports `bare-metal 0.2.5` (`RUSTSEC-2026-0110`) and
`proc-macro-error 1.0.4` (`RUSTSEC-2024-0370`) as unmaintained transitive
dependencies. These warnings remain open maintenance work in
[TODO.md](../TODO.md); the audit result is not a warning-free dependency bill.
Since 2026-10-10 the CI audit denies warnings and ignores exactly these two
advisories by ID ([auditing](code-quality.md#auditing)).

Later history: the record was written before the hardening changes were
committed as `2479c79` and last edited in `8a04b25` (both 2026-09-28). Since
`2479c79` and until the 2026-10-09 fixes, Rust sources, tests, Renode files,
and scripts changed only in comments that name renamed documents, so the
counts above described the tree until then; the checks were not re-run locally
for those changes. The fixes added tests, which the
[2026-10-09 record](#validation-record--2026-10-09) and the
[Test Map](testing.md#test-map) count. The 219 unit tests did not include the ten
`scanner.rs` tests, which were never compiled
([Tests That Do Not Run](testing.md#tests-that-do-not-run)). The hosted run on `2479c79` failed at the earlier actionlint installation step,
which `8a04b25` replaced. Hosted runs later passed all five check jobs on
`8a04b25` (run 36441995385, 2026-09-28) and on later commits; see
[Hosted CI Runs](testing.md#hosted-ci-runs). The tag-only packaging and release jobs
have still never run.

## Related Guides

- [Testing](testing.md)
- [Code quality](code-quality.md)
- [First flash](first-flash.md)
