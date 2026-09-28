# Work plan and completed work

This is the single project backlog. Open work is separate from implemented work
below. A checked item records code/documentation present in the repository; it
does not certify hardware compatibility or a production deployment. Validation
evidence belongs with its commit/release and [test record](docs/TESTING.md).

Priorities: **P0** blocks a managed production deployment; **P1** improves
reliability, interoperability, and maintainability; **P2** is a future feature.
Each open item includes a completion criterion so it can be closed with evidence.

## To do — P0: deployment and security gates

- [ ] **Authenticated pairing and enrollment policy.** Define whether each
  supported device uses authenticated pairing, how user presence is checked, and
  whether weaker devices are rejected. Add a bounded pairing window and visible
  state. Accept when downgrade, unsolicited pairing, timeout, and reconnect cases
  have automated tests plus representative real-device evidence.
- [ ] **Forget/reset hardware and interruption acceptance.** Validate the
  implemented confirmation, worker shutdown, persistence, and cache-update paths
  on a board. Accept when forgotten peers cannot reconnect across reboot, failed
  writes retain the documented state, and power interruption during both logical
  deletion and unreadable-store recovery has a tested outcome.
- [ ] **Provisioning and physical key protection.** Define production debug
  access, readout protection, key lifetime, disposal, and service recovery.
  Accept when the threat model and provisioning procedure are reviewed and
  readout/recovery behavior is demonstrated on a production-equivalent board.
- [ ] **USB production identity.** Obtain an assigned VID/PID and define product,
  manufacturer, revision, and unit-identity policy. Accept when descriptors and
  host inventory distinguish two units and development identifiers are absent
  from production builds.
- [ ] **Multi-device aggregation hardware acceptance.** Validate the implemented
  per-source key/modifier/button unions and consumer-slot priority with two real
  peripherals sharing an endpoint. Accept when releasing/disconnecting one source
  preserves the other's held state and the six-key rollover/consumer-priority
  behavior matches the documented policy on the USB host.
- [ ] **Backpressure behavior and bounded recovery.** Specify whether transient
  taps and accumulated motion may be lost while USB is stalled. Test sustained
  input, queue saturation, suspend, unplug, and a simultaneous BLE disconnect.
  Accept when final releases reach a recovered host within a measured bound,
  neither slot starves, and any intentional loss policy is documented.
- [ ] **Hardware compatibility baseline.** Run [FIRST_FLASH.md](docs/FIRST_FLASH.md)
  on declared keyboard/mouse models, Windows/Linux/macOS hosts, monitor hubs,
  and BIOS/UEFI or KVM targets. Accept when the matrix identifies exact versions,
  pass/fail results, known limitations, and the artifact hash.
- [ ] **Power-loss-safe persistence.** Fault-inject flash writes, garbage
  collection, malformed records, and version changes. Accept when interrupted
  writes retain the last committed valid state or take a documented safe reset
  path, without accepting corrupted bond material or silently replacing it.
- [ ] **Watchdog and recoverable failures.** Define progress-based watchdog
  feeding and recovery for stuck I2C, stalled USB, flash errors, and BLE task
  failure. Accept when injected faults cannot leave permanent held input or
  require an undocumented recovery sequence; record reset causes.
- [ ] **Memory and endurance budget.** Measure SoftDevice RAM at enable and
  worst-case stack high-water with two links, scanning, display, and persistence.
  Verify flash size and storage wear assumptions. Accept when reviewed margins
  are recorded and `memory_sd.x` matches the measured requirement; do not reduce
  its current 24 KiB reservation based only on estimates.
- [ ] **Hosted provenance and release recovery acceptance.** Run the configured
  tag/attestation workflow, verify its downloaded artifacts against the approved
  commit from a clean machine, and document storage migrations, rollback
  constraints, and service flashing. Accept when provenance verification and
  failed-update recovery have evidence; local helper tests and workflow lint do
  not exercise GitHub signing or device recovery.
- [ ] **Security maintenance ownership.** Publish a private reporting contact,
  supported-version policy, triage ownership, and response/update expectations.
  Accept when dependency/advisory review, license inventory, and remediation
  tracking are part of a documented release procedure.

## To do — P1: reliability and engineering quality

- [ ] **HID/USB conformance.** Define and implement supported `GET_REPORT` and
  `SET_IDLE` behavior, validate boot/report protocol transitions, and reject
  invalid control requests consistently. Accept when descriptor/request tests
  and USB captures from a real host demonstrate the supported behavior.
- [ ] **Descriptor-driven report translation.** Decode fields by usage, bit
  offset, width, signedness, and report ID for NKRO, packed mouse buttons, and
  16-bit movement. Accept when a fixture corpus covers supported layouts and
  unsupported layouts fail explicitly without misclassifying input.
- [ ] **Report Map interoperability and legacy policy.** Validate the implemented
  512-byte fragmented GATT reader against real peripherals with long maps and
  different MTUs. Decide whether the absent-map compatibility fallback remains
  allowed for deployment. Accept when captures demonstrate full reads and error
  handling, and same-length incompatible layouts are rejected by the supported
  descriptor-driven translation policy.
- [ ] **Parser fuzzing and property tests.** Add bounded fuzz targets for HID
  descriptors, advertisements, report classification, and persistence framing.
  Accept when CI runs a seed corpus and scheduled fuzzing records no panics,
  out-of-bounds access, excessive work, or invalid accepted output.
- [ ] **Async task fault tests.** Exercise command cancellation, full event
  channels, scan/reconnect contention, repeated security failures, and device
  disappearance during discovery. Accept when deterministic test cases assert
  completion, retry policy, and UI state without relying only on reducer tests.
- [ ] **Soak and latency measurements.** Run a defined multi-day keyboard/mouse
  workload with disconnects, monitor power changes, and flash updates. Accept
  when latency percentiles, reconnect time, dropped-input policy, reset count,
  and stack/flash margins meet published limits.
- [ ] **Bounded management wait in the UI.** While a saved-devices list, Forget,
  or reset request is pending, the UI ignores every button until the BLE
  coordinator replies. A hung coordinator therefore needs a power cycle. Define
  a deadline and a "result unknown, reopen saved devices" state that never
  claims success or failure it did not observe. Accept when reducer tests cover
  the lost-reply path and a late reply is rejected by its request ID.
- [ ] **Visible storage/security errors.** Surface pairing persistence failure,
  full-store replacement, unsupported reports, and security failures with useful
  user actions. Accept when UI tests cover every state and a user can distinguish
  a temporary link failure from a peer that was not saved.
- [ ] **Diagnostics without sensitive input.** Add firmware/build identification,
  reset reasons, bounded counters for reconnect/queue/write failures, and a
  documented collection method. Accept when reports support reproduction without
  logging key material or keystroke content.
- [ ] **Versioned storage migration.** Document each wire version and supported
  upgrade/downgrade paths. Accept when fixture tests reject unknown versions and
  cover valid legacy conversion, malformed data, and capacity changes.
- [ ] **Supply-chain and tooling maintenance.** Extend the dependency audit and
  update automation with license checks, an SBOM artifact, and
  verified digests for downloaded non-Cargo tooling/SoftDevice inputs. Accept
  when a license conflict or digest mismatch fails the relevant check with an
  actionable message and dependency alerts have an assigned review process.
- [ ] **Replace unmaintained transitive dependencies.** The 2026-09-28 audit
  reported `bare-metal 0.2.5` (`RUSTSEC-2026-0110`) and `proc-macro-error 1.0.4`
  (`RUSTSEC-2024-0370`) as unmaintained. Trace their dependency chains, track the
  upstream migration, and adopt maintained replacements through dependency
  upgrades. Accept when the lockfile no longer selects these affected versions,
  the audit is clean without advisory suppression, and host/firmware/simulation
  regression checks pass.
- [ ] **Reproducible firmware evidence.** Compare artifacts from two clean
  environments, document remaining nondeterminism, and enforce release size
  budgets. Accept when the release record identifies compiler, dependency,
  external-tool, source, and artifact hashes.
- [ ] **Development environment hardening.** Pin optional tools/container inputs
  and replace blanket container privilege with scoped probe access where
  feasible. Accept when fresh Linux/WSL setups pass checks and a no-probe setup
  still works.
- [ ] **Automated documentation checks.** Validate local links, command examples,
  and configuration/memory-map consistency in CI. Accept when a broken link or
  stale documented constant produces a targeted failure.

## To do — P2: product extensions

- [ ] **Multiple BLE profile sets.** Design selection, storage, migration, and
  connection ownership. Accept when switching profiles releases old inputs and
  only reconnects the selected profile's authorized peers.
- [ ] **Monitor-input-aware switching.** Identify an explicit supported signal
  from the monitor/host, define fallback behavior, and prototype against named
  hardware. Accept when input reaches the intended PC without leaking held keys
  during a switch.
- [ ] **Windows/macOS companion app.** Define a versioned, authenticated management
  protocol and installation/update policy before implementing the tray UI.
  Accept when settings, diagnostics, access control, and firmware compatibility
  are tested end to end.
- [ ] **Signed USB/BLE DFU.** Choose a bootloader and flash partition layout;
  implement signed image validation, rollback policy, interrupted-update recovery,
  and physical recovery. Accept only after power-cut and invalid-image tests.
- [ ] **Additional MCU/board targets.** Select a concrete target, isolate board
  configuration, and implement its radio/USB/storage integration. Accept when it
  has a maintained build and hardware acceptance record; alternatives listed in
  [Hardware](docs/HARDWARE.md) are not supported ports today.

## Done — implemented baseline

These items are supported by source and the available software tests. Board
validation remains separate even for a feature whose implementation is complete.

- [x] BLE central scan, GATT HID discovery, report-reference classification,
  bonding/encryption, and two connection slots (`src/ble/`).
- [x] Composite USB keyboard, mouse, and consumer interfaces, keyboard LED
  forwarding, remote-wakeup requests, and software VBUS event handling
  (`src/usb/hid_device.rs`).
- [x] Stored pairing/bond records, boot reconnect planning, identity-key matching,
  and retries after link loss (`src/storage.rs`, `src/ble/reconnect.rs`,
  `src/ble/multi_conn.rs`).
- [x] Per-source held-input aggregation and bounded coalescing/delivery policy
  with host-testable logic (`src/hid/aggregate.rs`, `src/hid/coalesce.rs`,
  `src/hid/delivery.rs`); residual load and hardware acceptance work is open above.
- [x] Five-button/scroll-capable mouse and consumer report types, alongside the
  six-key USB keyboard format (`src/hid/`).
- [x] Async OLED rendering, three debounced buttons, and activity-driven display
  power policy (`src/ui/`, `src/power_logic.rs`).
- [x] Pure shared host-test modules, unit/integration tests, and coverage tasks
  (`src/lib.rs`, `tests/`, `maskfile.md`).
- [x] SoftDevice-free Renode build, custom GPIO/GPIOTE models, and a headless
  GPIO/UI/coordinator scenario (`src/sim.rs`, `renode/`).
- [x] Board self-test and reusable first-flash checklist (`src/selftest.rs`,
  [FIRST_FLASH.md](docs/FIRST_FLASH.md)).
- [x] Separate application and pairing flash regions, linker assertions, and
  stack high-water instrumentation (`memory_sd.x`, `src/stack.rs`).
- [x] GitHub Actions build/test/simulation workflow and tag-based firmware
  artifact generation (`.github/workflows/ci.yml`).
- [x] WSL-aware task tooling and a VS Code devcontainer (`scripts/run-tool.sh`,
  `.devcontainer/`).
- [x] Reorganized setup/use, hardware, architecture, development, testing,
  operations, and security documentation; separated this backlog from completed
  work and removed unqualified compatibility/coverage claims (2026-09-28).

## Done — hardening changes

These entries describe implemented changes. The current change's validation
report must identify which software checks were executed; hardware gates above
remain open.

- [x] Bounded HID collection/global-state parsing, Push/Pop handling, malformed
  descriptor rejection, overflow-safe report-size arithmetic, and strict
  known-report routing (`src/hid/report_protocol.rs`, `src/hid/mod.rs`, descriptor
  and classification regression tests).
- [x] Consumer usage bounds, exact report-reference direction handling, rejection
  of oversized notifications, and distinct GATT payload classification
  (`src/hid/consumer.rs`, `src/ble/hid_client.rs`).
- [x] USB boot/report protocol negotiation, three-byte boot mouse serialization,
  keyboard LED control-request validation, reset state cleanup, and stable
  factory-derived per-unit USB serials (`src/usb/hid_device.rs`,
  `src/hid/mouse.rs`).
- [x] Peer-identity-scoped bond replacement and key lookup, stable identity
  persistence, and private-address resolution on background reconnect
  (`src/ble/multi_conn.rs`, `src/ble/scanner.rs`, `src/storage.rs`).
- [x] Require encrypted links before HID discovery, restrict application-initiated
  fresh pairing to explicit user connection attempts, and handle cancellation
  during owned-link security/discovery (`src/ble/multi_conn.rs`).
- [x] Validate complete storage frames and record metadata, reject unknown
  formats/types and malformed bond lengths, abort partial serialization, and
  preserve unreadable stores by disabling writes (`src/storage/`).
- [x] Preserve UTF-8 advertising names, merge scan-response names, and release
  the radio procedure lock before delivering UI scan results (`src/ble/`).
- [x] Scroll the discovered-device list to keep its selection visible, reject
  invalid selections, use a persistent UI ticker, and handle button levels and
  suspend-aware activity (`src/ui/`, `src/main.rs`, `src/power_logic.rs`).
- [x] Pin Rust/dependency resolution and BLE Git revision, align Cargo license
  metadata with the existing GPL license, and use locked build tasks
  (`rust-toolchain.toml`, `Cargo.lock`, `Cargo.toml`, `maskfile.md`).
- [x] Fix self-test flash ownership and pairing-record buffer capacity; reject
  combined firmware/simulation features (`src/selftest.rs`, `build.rs`).
- [x] Resolve WSL tool fallbacks only from the current Windows user's profile;
  fix the devcontainer build recipe and stop SoftDevice download/flash tasks on
  download or extraction failure (`scripts/run-tool.sh`, `maskfile.md`).
- [x] Configure Linux/Windows host checks, embedded/simulation lint and build,
  scheduled dependency auditing, pinned action revisions, restricted default
  workflow permissions, and draft release artifacts with checksums/build inputs
  (`.github/workflows/ci.yml`, `.github/dependabot.yml`). Workflow configuration
  still needs confirmation by a GitHub Actions run.
- [x] Stop devcontainer setup when required tool installation or its host-test
  smoke check fails (`.devcontainer/post-create.sh`); scoped probe permissions
  and optional-tool/container pinning remain open above.
- [x] Validate exact release tag/Cargo version equality, including prereleases;
  reuse the successful embedded build by immutable artifact ID; verify source,
  build-input and firmware hashes before packaging; configure SHA-pinned GitHub
  provenance attestations separately from draft publication permissions
  (`scripts/release.py`, `.github/workflows/ci.yml`). Twelve helper regression
  tests pass on Windows and WSL; actionlint 1.7.12 passes. Hosted issuance and
  downloaded-release verification remain open above.
- [x] Aggregate source-tagged input across two BLE slots, preserve the surviving
  source on disconnect, implement six-key rollover and deterministic consumer
  priority, and run independent bounded USB endpoint workers with retained-state
  replay and new-press-only wake (`src/hid/aggregate.rs`, `src/hid/delivery.rs`,
  `src/hid/wake.rs`, `src/usb/hid_device.rs`). Host regressions include actual
  asynchronous workers with fake endpoint sinks; hardware gates remain open.
- [x] Read complete GATT Report Maps by offset up to 512 bytes, distinguish
  missing maps from invalid/unreadable/oversized maps, and restrict legacy
  classification fallback to an absent characteristic (`src/ble/long_read.rs`,
  `src/ble/hid_client.rs`, `vendor/nrf-softdevice/README.bt2usb.md`).
- [x] Add saved-device management with default-Cancel confirmations, stable
  selected identities, worker quiescence before mutation, transactional cached
  bond updates, and explicit reset recovery of unreadable storage (`src/ui/`,
  `src/ble/management.rs`, `src/ble/multi_conn.rs`, `src/storage.rs`).
- [x] Retain errors and completion notices until acknowledgment; isolate OLED
  rendering from input/UI tasks, retry completed I2C failures, and request STOP
  after the display deadline without unsafely dropping an active DMA future
  (`src/main.rs`, `src/ui/display.rs`, `src/ui/ui_logic.rs`).
- [x] Tag saved-device list, Forget, and reset requests with a unique ID and
  allow one at a time, so a delayed or duplicate reply cannot complete a newer
  action (`src/ui/ui_logic.rs` `ManagementRequests`, `src/main.rs`,
  `src/ble/multi_conn.rs`). Reducer tests cover stale IDs and ID wraparound.
- [x] Hold TWIM NACK/overrun errors until the peripheral reports STOPPED. The
  pinned driver returns before STOPPED, which could release display-interface's
  transfer buffer during the stop sequence and let the retry clear the pending
  event (`src/ui/display.rs` `StopSafeI2c`, also used by the self-test).
- [x] Remove peer-triggerable panics and unbounded loops from vendored GATT
  discovery: resume after a truncated characteristic response, reject
  descriptor overflow and out-of-range, non-advancing, or empty responses, and
  saturate handle arithmetic. HID discovery failures now log the cause
  (`vendor/nrf-softdevice`, `src/ble/hid_client.rs`). Needs real-peripheral
  evidence under the Report Map interoperability item above.
- [x] Stage release inputs and Renode results in the runner's temporary
  directory instead of the Rust-cached `target/`. Fail the release job before
  upload when the tag's release is already published; reruns may update only a
  draft. Action pin comments name the exact upstream tag each SHA resolves to
  (`.github/workflows/ci.yml`, [Releasing](docs/RELEASING.md)).

## Closing an item

Move the item into the appropriate completed section with the implementation
reference and the evidence satisfying its acceptance criteria. Record tests that
were skipped. A release gate depending on a physical board stays open until its
hardware evidence exists, even if supporting code is merged.
