# ADR 0018: Identify Releases With A pid.codes Product ID And Units By Their Factory Serial

- Status: Proposed
- Date: 2026-10-10

## Context

Hosts, monitor hubs, and KVM switches know the bridge only through its USB
descriptors. The device descriptor (USB 2.0 Specification, section 9.6.1,
Table 9-8) carries `idVendor`, `idProduct`, the BCD release number
`bcdDevice`, and the manufacturer, product, and serial string indexes (section
9.6.7). Hosts remember devices under these fields, so the fields decide
whether a computer the bridge comes back to, after a KVM switch or a port
change, recognizes it as the keyboard it already knows.

**What the bridge sends today** (tree at `287fb96`):

- `USB_VID` `0x1209`, `USB_PID` `0x0001`, manufacturer `bt2usb`, and product
  `BT-to-USB HID Bridge` ([config.rs](../../src/config.rs) lines 102 to 109).
  pid.codes titles PIDs `0x0001` to `0x0010` "pid.codes Test PID", and its page
  for `0x0001` says it "MUST NOT be used on any device that will be
  redistributed, sold, or manufactured" (checked 2026-10-10).
- `init` in [hid_device.rs](../../src/usb/hid_device.rs) calls
  `Config::new(USB_VID, USB_PID)` at line 272 and builds the serial at lines
  275 to 288: `FICR.DEVICEID[1]` then `DEVICEID[0]`, each `{:08X}`, 16
  uppercase hex characters in the `USB_SERIAL` static (line 68). The nRF52840
  Product Specification's FICR chapter calls these words (`0x10000060`,
  `0x10000064`) a "64 bit unique device identifier"; factory-programmed and
  read-only, they survive reflashing, full erase, and port changes. Nothing
  else in `src/` or the vendored `nrf-softdevice` reads them.
- `init` sets remote wakeup and restates two defaults, `max_power` (100 mA)
  and `max_packet_size_0` (64). The rest are `Config::new` defaults of the
  pinned `embassy-usb` 0.6.0 (`src/builder.rs`): `bcdDevice` `0x0010` (`lsusb`
  shows `0.10`) whatever the firmware version, `bcdUSB` `0x0210` with a BOS
  holding only a USB 2.0 Extension capability, bus-powered, and class
  `0xEF`/`0x02`/`0x01` with an Interface Association Descriptor before each
  single-interface HID function; the configuration descriptor carries
  `bmAttributes` `0xA0` and `bMaxPower` 50 (2 mA units). The crate's encoders
  (`src/descriptor.rs`) are `pub(crate)`, and an upgrade that changed a
  default would change what hosts see.
- Interfaces 0 and 1 are the boot keyboard and mouse (lines 325 and 337), and
  `BootRequestHandler` in [host_requests.rs](../../src/usb/host_requests.rs)
  answers `SET_PROTOCOL` for firmware setup screens (HID 1.11, 4.2 and 7.2.6).
  The self-test reuses `init` ([selftest.rs](../../src/selftest.rs) line 133).
- A string over 62 UTF-16 code units does not fit the 128-byte `USB_CTRL_BUF`
  and trips an `embassy-usb` assertion at enumeration. No Microsoft OS
  descriptor is offered: no handler overrides `get_string`, so a request for
  string `0xEE` is rejected (stalled).
- Nothing on the device reports the firmware version
  ([version lifecycle](../deployment.md#version-lifecycle)), and
  `release.py stage` records `features` as the fixed value `["embedded"]`, so
  nothing marks a release image ([build policy](../deployment.md#version-and-build-policy)).

**How hosts key on these fields** (vendor documentation where named, otherwise
general knowledge):

- **Windows** names a device instance from VID, PID, and serial. Microsoft's
  "USB device registry entries" page says `IgnoreHWSerNum` means "the device
  instance is tied to the port", as for any device without a serial. The
  instance holds "Allow this device to wake the computer", which remote wakeup
  needs ([first flash, step 5](../first-flash.md#5-in-the-monitor)). The page
  keys `usbflags\vvvvpppprrrr` on VID, PID, and `bcdDevice`, whose `osvc` value
  caches the Microsoft OS descriptor answer. Hardware IDs carry `REV_` from
  `bcdDevice`. Inbox drivers bind HID by interface class, for any VID and PID.
- **macOS** opens Keyboard Setup Assistant for a keyboard it cannot identify
  (Apple's "Specify a keyboard type" help). A Mac OS X 10.5-era forum report
  shows the answer in `com.apple.keyboardtype.plist` under `16-4176-0`, a
  YubiKey's product and vendor IDs in decimal; modifier remaps are also kept per
  VID and PID (current macOS not checked). A new VID or PID asks every Mac
  again, and two bridges on one Mac share one answer.
- **Linux** names input devices from the manufacturer and product strings, puts
  the serial in `/dev/input/by-id` links and world-readable sysfs, and matches
  hwdb and quirk entries on VID and PID.
- **KVMs.** A monitor hub's KVM switches its upstream port, so the selected
  computer enumerates the bridge afresh. Whether emulating KVMs key behavior on
  VID and PID is vendor-specific; none is in the hardware baseline yet.
- **UEFI.** EDK II's `UsbKbDxe` binds by interface class, subclass, and
  protocol (3/1/1), selects the boot protocol, and reads no VID, PID, or
  string; vendor firmware was not checked.

**What a serial costs.** Many commodity keyboards report none (general
knowledge). The serial lets software on every host recognize the unit and
correlate it across computers, for example an endpoint agent's USB inventory;
web pages cannot, as WebHID exposes no serial (also general knowledge). The
[threat model](../security.md#threat-model) lists "Fingerprint or track the
unit" without mitigation, yet [USB production identity](../../TODO.md#usb-hid-device)
accepts only when "descriptors and host inventory distinguish two units".

**Where a VID and PID can come from** (checked 2026-10-10):

- **pid.codes** gives PIDs under `0x1209` to projects with a public repository
  of modifiable source code or PCB files for a USB device, under a recognized
  open-source or open-hardware license, with a LICENSE file. For a project
  that is only software or only hardware "we may ask for further
  justification as to why you need a PID"; a project with both must license
  both. A request is a pull request adding `org/<name>/index.md` and
  `1209/<PID>/index.md`. bt2usb is public on GitHub, `GPL-3.0-only` with a
  LICENSE file, and runs on an nRF52840-DK; no production board is chosen
  ([production hardware definition](../../TODO.md#board-bring-up-and-hardware-acceptance)).
- **USB-IF** sells a vendor ID for US$6,000, or includes one in a US$5,000
  yearly membership; the USB logo also needs a logo license and testing.
- **Nordic** (VID `0x1915`, general knowledge): a Nordic engineer's DevZone
  answer ("usb pid for nrf52840", thread 50638) says Nordic has no system for
  registering PIDs, that small series and hobbyists use its VID but "it is at
  your own risk", and that "if many customers use Nordic's VID, there will be
  collisions".

## Decision

**Request one PID from pid.codes now**, as a GPL-3.0 firmware project whose
reference hardware is the documented nRF52840-DK wiring, saying so in the
request because pid.codes may ask a software-only project for justification;
add an open-hardware license if a custom board is chosen later. The allocated
value becomes `USB_PID_RELEASE`; nothing ships with it before the merge.

**Define two identities, and give the release one only to tag builds.**

| Field | Release build | Development build | Source |
| --- | --- | --- | --- |
| `idVendor` | `0x1209` | `0x1209` | `USB_VID` |
| `idProduct` | The allocated PID | `0x0001` (pid.codes test PID) | `USB_PID_RELEASE`, `USB_PID_DEVELOPMENT` |
| `bcdDevice` | Cargo `MAJOR` and `MINOR` as two BCD bytes | Same | `usb::identity::bcd_device` |
| Manufacturer | `bt2usb` | `bt2usb` | `USB_MANUFACTURER` |
| Product | `BLE HID Bridge` | `BLE HID Bridge (development)` | `USB_PRODUCT_RELEASE`, `USB_PRODUCT_DEVELOPMENT` |
| Serial | 16 uppercase hex characters from `DEVICEID[1]`, `DEVICEID[0]`, as today | Same | `usb::identity::format_serial` |

`build.rs` reads `BT2USB_RELEASE_TAG`; when it is not empty it must equal `v`
plus `CARGO_PKG_VERSION`, or the build fails, and `build.rs` emits
`cargo:rustc-cfg=bt2usb_release` (declared with `rustc-check-cfg`) and reruns
whenever the variable changes, so no cached build crosses channels. The
`embedded` job in `ci.yml` sets it to `github.ref_name` on `v*` tags only, as
a job-level variable so its Clippy, rustdoc, and build steps compile the same
cfg; every other build gets the development identity.

**Check the shipped bytes.** The active identity is also a 16-byte record: the
magic `BT2USBID`, a format byte (1), a channel byte (0 development, 1 release),
and VID, PID, and `bcdDevice`, little-endian. `init` builds `Config` from one
volatile read of it (a documented `unsafe` block), so LTO and `--gc-sections`
keep it and it is what the device enumerates with. `release.py stage` requires
exactly one record in the loadable segments of each shipped ELF and writes it
to `BUILD-INFO.json` as `usb_identity` (`schema_version` 2). `release.py
package` rereads both ELFs and `bt2usb.hex` and refuses the release unless the
channel is release, VID and PID equal the allocated pair (so the PID is
outside the test range `0x0001` to `0x0010`), `bcdDevice` matches the tag, and
the metadata agrees.

**`bcdDevice` names the release line:** BCD `MAJOR` in the high byte and BCD
`MINOR` in the low byte, so `0.1.0` gives `0x0001` (`lsusb` shows `0.01`) and
`1.12.3` gives `0x0112`; a compile-time assertion keeps both at most 99.
`0.2.0-rc.1` and `0.2.0` share `0x0002`. A change to any descriptor, string,
interface, or answered request, a first Microsoft OS descriptor included,
ships in a new `MINOR` or `MAJOR`.

**Set every `embassy_usb::Config` field explicitly, at today's values**:
`bcdUSB` `0x0210`, class `0xEF`/`0x02`/`0x01` with `composite_with_iads`,
`max_packet_size_0` 64, bus-powered, `max_power` 100 mA per
[ADR 0012](0012-bus-powered-no-system-off.md), and remote wakeup. Pure
`device_descriptor` (18 bytes) and `config_attributes` (`bmAttributes`,
`bMaxPower`) give the bytes host tests pin; the crate's encoders are
`pub(crate)`, so a hardware dump compares them with what is sent. The boot path
is unchanged: interfaces 0 and 1 stay the boot keyboard and mouse, and no
changed field takes part in class binding. Whether a setup screen or KVM needs
class `0x00` without IADs is for the hardware baseline to show.

**Keep the factory serial in both channels**, unchanged, so units already
recorded keep theirs. It is never logged; boot logs
`USB identity: {:04x}:{:04x} rev {:04x} ({})` with the channel instead.

**Strings.** The manufacturer stays `bt2usb`; the product becomes
`BLE HID Bridge`, which names what the bridge accepts (Bluetooth Classic is out
of scope), avoids the Bluetooth word mark, does not repeat the manufacturer
that Linux prepends, and stays short for setup screens and KVM menus. A
compile-time assertion holds each string to 62 UTF-16 code units.

### Open Questions For The Owner

1. **VID and PID source:** `pidcodes` (recommended), `usbif`.
2. **Serial in release builds:** `factory` (recommended), `none`, `resettable`.
3. **Product string:** `rename` to `BLE HID Bridge` (recommended), `keep`.
4. **When to request the PID:** `now` (recommended), `after-hardware`.

## Alternatives Considered

- **Keep `0x1209:0x0001`.** It breaks the pid.codes rule, and host state or
  Linux rules made for any other test device would apply to the bridge.
- **A USB-IF vendor ID.** It buys 65,536 PIDs and logo eligibility, but the
  bridge needs one PID and no logo. It becomes right if the bridge is sold
  under a company name, at the cost of one more identity change.
- **Nordic's VID with a self-chosen PID.** Collisions can be neither ruled out
  nor detected, and Nordic keeps no registry and leaves the risk with the user.
  Openmoko's community registry under `0x1D50` (general knowledge; its current
  terms were not checked) would be a similar route, whereas pid.codes was
  checked.
- **Another vendor's IDs**, such as Apple's, which the same forum report says
  hides Keyboard Setup Assistant. That impersonates the vendor and binds its
  quirks (Linux `hid-apple` remaps function keys for Apple IDs).
- **No serial.** Windows then ties the instance to the port, so another port or
  dock makes a new device with default wake permission; two units share Linux
  `by-id` names; and inventory cannot tell units apart.
- **A hashed or a resettable serial.** A hash of `DEVICEID` tracks across
  hosts just as well; it only stops a probe read of FICR matching a unit to its
  host entries. A random serial renewed by Factory reset would let an owner
  break the link before passing a unit on, but needs a settings record outside
  the fail-closed pairing frame ([ADR 0006](0006-fail-closed-pairing-store.md)),
  SoftDevice randomness, and a rule for an unreadable store, which would
  otherwise change the unit's identity on every host.
- **Another `bcdDevice` scheme.** The default says nothing and follows the
  dependency; four BCD digits cannot hold `MAJOR.MINOR.PATCH` at useful
  ranges; a build counter is neither tag-bound nor reproducible.
- **A PID per release or interface set.** Each upgrade would re-ask every Mac
  and create new Windows instances, though hosts reread the configuration
  descriptor at each enumeration. Identifying a configuration a running unit
  switches to (the compatibility mode) is left to the USB extensions ADR.
- **Other guards.** The release identity in every build would let development
  images, which carry the last release's version, claim its identity. A Cargo
  feature is easily forgotten or set locally, and `stage` records features as
  a fixed value; a build warning fails nothing.

## Rationale

pid.codes fits a GPL-3.0 project that needs one PID: it is free, its rules are
public, and hosts treat its PIDs like any other. "The PID names the product,
`bcdDevice` the release line, the serial the unit" follows how hosts remember
devices: a Windows instance survives upgrades because `bcdDevice` is not part
of it, a Mac asks once per VID and PID, and `REV_` and `lsusb` finally show
which release a unit runs.

A wired keyboard with a serial keeps one entry, with its wake permission, on
each computer whatever port or dock it uses, as the product promises; the
serial is also the only way to meet the acceptance criterion, and question 2
lets the owner decline its tracking cost. The guard fails closed at build and
at packaging and inspects the flashed bytes, as
[ADR 0008](0008-attested-draft-releases.md) trusts digests over descriptions.
All rules are host-tested pure functions ([ADR 0003](0003-pure-core-and-task-shell.md)).

## Consequences

Positive:

- Released units carry their own PID, show their release line, and are told
  apart; each keeps its Windows instance and wake permission across ports,
  docks, and KVM switching.
- The descriptor no longer depends on `embassy-usb` defaults, and development
  images are visibly marked.

Negative:

- Every host meets the release identity as a new device once: a new Windows
  instance with default wake permission, one more Keyboard Setup Assistant per
  Mac, and Linux rules for `1209:0001` stop matching. Switching a unit between
  development and release images, or to a later USB-IF VID, repeats this.
- Two bridges on one Mac share one keyboard type, and the serial stays a
  cross-host identifier: the threat row becomes an accepted risk.
- A descriptor fix cannot ship as a patch release; versions stop at `99.99`.
- `release.py` gains an ELF and HEX reader and `BUILD-INFO.json` schema 2,
  amending ADR 0008's packaging checks; reproducible-build comparisons must set
  `BT2USB_RELEASE_TAG` alike; the USB shell adds a seventh `unsafe` block to
  the [inventory](../security.md#unsafe-code). WebHID pickers filter by VID and
  PID (general knowledge), so the planned browser page must list both PIDs.

Product promise: inbox class drivers still bind, so no host needs software;
EDK II firmware setup ignores every changed field (vendor firmware is
unverified); a monitor-hub KVM re-enumerates the same identity on each
computer; emulating KVMs remain unverified. Estimated cost: about 16 bytes of
flash for the record and under 100 for the log line and `Config` copies, no
new RAM, and about 80 lines of Python.

## Implementation

| Concern | Where |
| --- | --- |
| Constants | `USB_VID`, `USB_PID_RELEASE`, `USB_PID_DEVELOPMENT`, `USB_MANUFACTURER`, `USB_PRODUCT_RELEASE`, `USB_PRODUCT_DEVELOPMENT` in [config.rs](../../src/config.rs), replacing `USB_PID` and `USB_PRODUCT` |
| Pure identity | New `src/usb/identity.rs`, declared in [usb/mod.rs](../../src/usb/mod.rs) and in the host library through `#[path]` like `ble/conn_params.rs`: `Channel`, `UsbIdentity`, the active identity under `cfg(bt2usb_release)`, a const parser of `CARGO_PKG_VERSION_MAJOR`/`MINOR`, `bcd_device`, `format_serial`, `IdentityRecord::encode`, `device_descriptor`, `config_attributes`, string-length assertions |
| Channel | `BT2USB_RELEASE_TAG` and `bt2usb_release` in [build.rs](../../build.rs); the job-level variable on the `embedded` job in [ci.yml](../../.github/workflows/ci.yml) |
| Shell | `init` in [hid_device.rs](../../src/usb/hid_device.rs): read the record, set every `Config` field, call `format_serial`, log the identity |
| Release check | `stage` and `package` in [release.py](../../scripts/release.py), with the allocated pair as a constant; one ELF reader and one schema bump shared with [ADR 0021](0021-provisioning-debug-access-and-readout-protection.md) (Proposed); `release_test.py` fixtures become minimal ELF and HEX images carrying a record, as today's are text payloads |
| Guides | The USB identity table in [data-model.md](../data-model.md#usb-device-identity); the new constants in [hardware.md](../hardware.md#configuration-defaults), which `check_docs.py` requires for every `config.rs` constant; `lsusb -d 1209:0001` in [operations.md](../operations.md#runtime-surfaces); the assets, threat, and unsafe rows of [security.md](../security.md#threat-model); `BUILD-INFO.json` in [deployment.md](../deployment.md#artifact-flow) |

Host tests (new `src/usb_identity_tests.rs`, included from `src/lib.rs` under
`cfg(test)`) cover `bcd_device` for `0.1.0`, `1.12.3`, `99.99.0`, and `None`
at 100; the version parser; `format_serial` for zero and all-ones words,
leading zeros, uppercase, word order, and only characters Windows accepts
(`0x20` to `0x7F`, no comma; general knowledge); the record layout; both
channels' device descriptors and configuration attributes; string lengths;
and a release PID outside `0x0001` to `0x0010`. Release helper tests cover a
missing, duplicated, development, or test-PID record, a `bcdDevice` or
metadata that disagrees with the tag or image, and the allocated pair matching
`config.rs`. Renode does not apply (the simulation build has no USB). Hardware
evidence ([ADR 0004](0004-layered-verification.md)): `lsusb -v` of both
channels, compared byte for byte with the pinned descriptors; on Windows, the
instance ID `USB\VID_1209&PID_<pid>\<serial>` and a hardware ID with `REV_`,
with the same instance and wake permission after a port change; one Keyboard
Setup Assistant on macOS; two units listed separately on one host; and
first-flash step 5 (firmware setup, cold start, monitor KVM) on a release.

This unblocks "USB production identity" and the
[release gate](../deployment.md#release-gates) "Assigned USB identity", and
gives "ADR: USB interface and report extensions" and "ADR: host management
interface" their rules for a changed interface set and a Microsoft OS
descriptor. ADR 0021 records the serial at provisioning and would decide a
label.

### Verification Status

- **Implemented:** nothing of this proposal. Today `config.rs` holds the
  development identity, `init` builds the FICR serial, and every `Config`
  field it does not set is an `embassy-usb` default.
- **Software-verified:** no host test covers the USB identity or the serial
  format, and none of the 17 release helper tests inspects an identity; `init`
  passes embedded Clippy with warnings denied (2026-10-10 validation record in
  [testing](../testing.md#validation-record--2026-10-10)).
- **Hardware-verified:** not yet. No descriptor dump is recorded, and the
  first-flash unit-identity step has no result.

## Related

- [Data model: USB device identity](../data-model.md#usb-device-identity), [features: USB HID device](../features.md#usb-hid-device),
  [hardware: configuration defaults](../hardware.md#configuration-defaults), [operations: runtime surfaces](../operations.md#runtime-surfaces)
- [Security: assets](../security.md#assets), [threat model](../security.md#threat-model), [USB host interface](../security.md#usb-host-interface),
  [unsafe code](../security.md#unsafe-code); [code quality: unsafe code policy](../code-quality.md#unsafe-code-policy)
- [Deployment: version lifecycle](../deployment.md#version-lifecycle), [release gates](../deployment.md#release-gates);
  [testing: release helper tests](../testing.md#release-helper-tests); [first flash: in the monitor](../first-flash.md#5-in-the-monitor);
  [architecture: decisions needed](../architecture.md#decisions-needed-for-roadmap-work)
- [ADR 0003](0003-pure-core-and-task-shell.md), [ADR 0004](0004-layered-verification.md), [ADR 0006](0006-fail-closed-pairing-store.md),
  [ADR 0008](0008-attested-draft-releases.md), [ADR 0012](0012-bus-powered-no-system-off.md),
  [ADR 0021](0021-provisioning-debug-access-and-readout-protection.md) (Proposed, shares the release ELF reader)
- TODO.md: [USB HID device](../../TODO.md#usb-hid-device), [shared decisions](../../TODO.md#shared-decisions),
  [hand-off between hosts and KVMs](../../TODO.md#hand-off-between-hosts-and-kvms), [updates and host tools](../../TODO.md#updates-and-host-tools),
  [device security and provisioning](../../TODO.md#device-security-and-provisioning), [release, provenance and supply chain](../../TODO.md#release-provenance-and-supply-chain)
