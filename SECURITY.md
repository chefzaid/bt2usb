# Security Policy

bt2usb is development firmware, not a hardened or certified input appliance.
The [security reference](docs/security.md) describes its assets, trust
boundaries, threat model, implemented controls, and known limitations.

## Scope

In scope:

- the firmware in this repository: the bridge, the self-test image, and the
  shared code under `src/`, including the vendored `nrf-softdevice` patch in
  `vendor/`
- the build and release path: `.github/workflows/ci.yml`, `scripts/`, the
  tasks in `maskfile.md`, and the attested release packages it is configured
  to produce
- documentation that tells people how to verify, flash, or recover a unit

Out of scope; report these to the component's own maintainer instead:

- Nordic's SoftDevice S140 binary and the BLE link layer it implements
- host operating systems, their HID drivers, and USB hubs or monitors
- firmware of the BLE keyboards and mice being bridged
- vulnerabilities in third-party crates or GitHub Actions; tell us as well if
  bt2usb's use of one is affected

Limitations already listed in the
[security reference](docs/security.md#security-posture-summary), such as Just
Works pairing or the open debug port, are known open work. A report that shows
new impact from one of them is still welcome.

## Supported Versions

There is no published support lifetime or security-response SLA yet. Fixes land
on `main`; use the latest commit there, since no release tag has been published
yet. The open release gates in [TODO.md](TODO.md) track the work needed before
managed deployment, including a security contact and a supported-version policy
([Security maintenance ownership](TODO.md#release-provenance-and-supply-chain)).

## Reporting A Suspected Vulnerability

GitHub private vulnerability reporting (the **Report a vulnerability** button on
the repository's **Security** tab) is not enabled for this repository yet
(checked on 2026-10-09), and no private security contact is published. Until
one is, open a public issue that only asks a maintainer for a private reporting
channel; do not include the affected component, exploit details, logs, or
secrets in it. If the **Report a vulnerability** button appears later, use it
instead.

A useful report includes the affected commit/release, hardware and peripheral
models, reproducible steps, observed impact, and a minimal sanitized reproducer.
State the `DEFMT_LOG` level of any log you send. Never attach LTKs, IRKs, raw
flash dumps, or captured private keystrokes to a public issue, and never share
a log from a `trace`-level build, which can contain raw keystroke reports. For
ordinary functional bugs, include only the relevant sanitized RTT log and the
failed acceptance step.

Please keep details private until a fix or mitigation is available; there is
no committed disclosure timeline yet.
