# Security

bt2usb is development firmware, not a hardened or certified input appliance.
There is no published support lifetime or security-response SLA. The
[open release gates](TODO.md) track work needed before managed deployment.

## Trust boundaries and current limits

The bridge accepts data from nearby BLE peripherals and can inject keyboard and
mouse input into its USB host. A paired device is therefore trusted to act as a
local input device. Do not pair an untrusted device with a privileged host.

The current BLE security handler declares `IoCapabilities::None`. It supports
bonding and encrypted links but has no passkey or numeric-comparison user flow.
Do not treat this pairing method as authenticated protection against a nearby
active attacker. There is no enterprise enrollment/allowlist policy or bounded
pairing-mode policy beyond the existing device-selection UI.

HID discovery waits for an encrypted link; signed-only modes are not sufficient.
The application initiates new pairing only for an explicit user connection, not
a background reconnect. Bond lookups and updates are tied to peer identity, not
just the encryption master ID. These controls do not add MITM authentication to
Just Works pairing.

Long-term encryption and identity keys are stored in the nRF52840's internal
flash as part of the paired-device record. This application does not encrypt
those records at rest or provision a production debug/readout-protection policy.
Physical debug access and firmware artifacts that contain memory dumps must be
treated as sensitive. The UI's disconnect action does not erase stored bonds.
Malformed, unsupported, or unreadable stores disable ordinary persistence writes
to avoid silently overwriting existing keys. The saved-device menu provides
separate Forget and Factory reset confirmations, defaulting to Cancel. A
successful operation commits storage before changing cached bonds. Reset can
explicitly recover an unreadable region. These are logical device-management
operations, not certified physical key erasure; readable-store updates can leave
older flash records until garbage collection. A reported storage failure must
not be treated as successful deletion or enrollment.

Firmware updates currently use a debug probe. The application provides no signed
update verifier, anti-rollback mechanism, secure boot chain, or OTA/USB DFU flow.
The configured release workflow signs GitHub artifact provenance for the checked
build and its checksum manifest. Verify the expected source and workflow identity
using [Release provenance](docs/RELEASING.md); an unauthenticated checksum alone
does not establish origin. Hosted signing and verification still need a successful
tag-run acceptance record. Artifact attestations do not make the device enforce
signed firmware. Production USB identity, protected provisioning, and recovery
are open tasks.

## Reporting a suspected vulnerability

Use the repository host's private vulnerability-reporting facility if maintainers
have enabled one. Otherwise request a private reporting channel from a maintainer
without posting exploit details or secrets publicly. This repository does not
currently designate a security contact or response deadline; establishing those
is tracked in TODO.md.

A useful report includes the affected commit/release, hardware and peripheral
models, reproducible steps, observed impact, and a minimal sanitized reproducer.
Never attach LTKs, IRKs, raw flash dumps, or captured private keystrokes to a
public issue. For ordinary functional bugs, include only the relevant sanitized
RTT log and the failed acceptance step.

## Contributor expectations

Keep untrusted report lengths, IDs, descriptor fields, and persistence records
bounded and validated before use. Test malformed and truncated inputs alongside
normal cases. Security-sensitive changes need a review of pairing policy,
key lifetime, release behavior, and reset/recovery paths, plus hardware evidence
where host tests cannot exercise the driver behavior.

Do not log bond keys or keystroke contents to aid diagnostics. Review dependency
and build-tool updates before release. Link security decisions and exceptions to
the applicable [release gate](docs/OPERATIONS.md), rather than describing an
unfinished control as implemented.
