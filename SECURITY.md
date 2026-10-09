# Security Policy

bt2usb is development firmware, not a hardened or certified input appliance.
The [security reference](docs/security.md) describes its trust boundaries,
implemented controls, and known limitations.

## Supported Versions

There is no published support lifetime or security-response SLA yet. Fixes land
on `main`; use the latest commit or release. The open release gates in
[TODO.md](TODO.md) track the work needed before managed deployment, including a
security contact and a supported-version policy.

## Reporting A Suspected Vulnerability

Use the repository host's private vulnerability-reporting facility if maintainers
have enabled one. Otherwise request a private reporting channel from a maintainer
without posting exploit details or secrets publicly.

A useful report includes the affected commit/release, hardware and peripheral
models, reproducible steps, observed impact, and a minimal sanitized reproducer.
Never attach LTKs, IRKs, raw flash dumps, or captured private keystrokes to a
public issue. For ordinary functional bugs, include only the relevant sanitized
RTT log and the failed acceptance step.
