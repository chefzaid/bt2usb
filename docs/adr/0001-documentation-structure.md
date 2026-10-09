# ADR 0001: Keep The README Short And Organize Detailed Docs By Reader

- Status: Accepted
- Date: 2026-10-09

## Context

bt2usb's documentation grew with the firmware, one layer at a time:

| Date | Commit | Documentation state |
| --- | --- | --- |
| 2026-02-21 | `8e6dd17` | The embedded implementation lands with a 379-line README and `maskfile.md` as the only documents |
| 2026-09-26 | `f477d4c` | The README reaches 466 lines; `docs/FIRST_FLASH.md` is added for board bring-up |
| 2026-09-28 | `2479c79` | The README shrinks to 100 lines; upper-case guides (`ARCHITECTURE`, `DEVELOPMENT`, `FIRST_FLASH`, `HARDWARE`, `OPERATIONS`, `RELEASING`, `TESTING`), a root `SECURITY.md` and a root `TODO.md` appear |
| 2026-10-09 | `7fc99d6` | This decision: lower-case guides by reader, a feature catalog, a data-model and security reference, and an ADR log |

Until 2026-09-26 the README mixed positioning, the bill of materials, pin
mapping, the memory map, the Embassy task model, build and flash steps, the
Renode guide, the user flow, a roadmap, and a comparison of alternative MCUs.
The 2026-09-28 split helped, but it left four problems:

- `SECURITY.md` mixed the short reporting policy that GitHub surfaces with the
  security design, so neither was easy to find.
- `OPERATIONS.md` mixed release gates and flashing with the recovery runbook.
- There was no catalog of what the firmware does today, no reference for the
  persisted pairing store or the USB report contracts, and no durable record of
  why the firmware is built the way it is. Rationale lived in commit messages
  (for example the removal of flip-link in `f477d4c`) and in code comments.
- The upper-case names differed from the owner's sibling project.

That sibling project, swirl-demo-app, uses a short README, reader-oriented
lower-case guides in `docs/`, and an ADR log. Following the same shape lets both
repositories be read the same way. Its web-application topics (Kubernetes,
SonarQube, DNS, Kafka, browser tests) do not apply to firmware and are not
copied; bt2usb adds the firmware-specific guides it needs instead (hardware,
first flash, data model).

The project owner also set two rules: `TODO.md` lists every task, with the
finished ones checked off and the open ones unchecked, and the README stays
simple because the detail lives in the guides.

## Decision

Keep `README.md` as a short entry point and give every other topic one home,
chosen by who reads it:

| File | Reader and purpose |
| --- | --- |
| `README.md` | Everyone: what the device is, its status, the documentation map, a quick start, the roadmap pointer, and the license |
| `TODO.md` | Everyone tracking progress: the complete checklist of work, with done tasks checked off and their source references, and open tasks unchecked with a priority and an "Accept when" criterion; it is not a remaining-work list |
| `SECURITY.md` | Someone reporting a vulnerability: the short policy GitHub links from its security tab |
| `maskfile.md` | Contributors: the executable task reference (`mask <task>`) |
| `docs/features.md` | Evaluators and users: the catalog of implemented capabilities, the controls, and the current technical boundaries |
| `docs/architecture.md` | Design reviewers: runtime architecture, constraints, the ADR process, and the ADR index |
| `docs/data-model.md` | Anyone changing a persisted format or a contract: the pairing store, the USB device identity, the HID report contracts, task channels and internal message contracts, error tags and the UI messages they map to, the UI state model, data ownership, and schema change rules |
| `docs/hardware.md` | Builders: parts and wiring, buttons, OLED, USB, power, clocks and radio, compile-time configuration, the memory map, and porting to another nRF52840 board |
| `docs/development.md` | Contributors: toolchain, build configurations and log levels, commands, devcontainer and WSL, and how to make a change |
| `docs/code-quality.md` | Contributors and reviewers: the quality gates every change passes, lint and unsafe-code policy, coverage policy, dependency hygiene, size budgets, and the review checklist |
| `docs/testing.md` | Contributors and release reviewers: verification layers, how to run tests and coverage, CI, and dated validation records |
| `docs/first-flash.md` | Whoever brings up a board: the self-test and the hardware acceptance checklist |
| `docs/deployment.md` | Whoever ships a build: the release pipeline, provenance verification, flashing, and release gates |
| `docs/operations.md` | Whoever runs or debugs a unit: the recovery and diagnostics runbook |
| `docs/security.md` | Security reviewers: trust boundaries, implemented controls, what each log level reveals, and limitations |
| `docs/adr/NNNN-*.md` | Design reviewers: one architecture decision record per decision |

Two supporting documents keep their place next to what they describe:
`vendor/nrf-softdevice/README.bt2usb.md` records the vendored patch
([ADR 0007](0007-vendored-softdevice-patch.md)), and
`.github/ISSUE_TEMPLATE/` holds the bug, feature, and hardware-result
templates.

Writing rules for all of them:

- **Implemented versus planned.** `features.md` lists only what the source
  implements. Planned work lives in `TODO.md` and is never described as a
  feature because a dependency, a constant, or a partial implementation exists.
- **Verification level.** Every claim says how far it is verified: implemented,
  software-verified (host tests, builds, Renode), or hardware-verified (a
  recorded [first-flash](../first-flash.md) result), as defined in
  [ADR 0004](0004-layered-verification.md).
- **One home per fact.** Every fact is written in full in one place, and
  other documents link to that home instead of repeating it. A document that
  needs the fact for context keeps the heading and a short paragraph with a
  link. The homes are:
  - constants, pins, and the memory map: `hardware.md`
  - byte layouts, message contracts, error tags and the UI messages they
    map to: `data-model.md`
  - commands: `development.md` and `maskfile.md`
  - log levels: `development.md` for how `DEFMT_LOG` selects them, and
    `security.md` for what each level reveals
  - coverage: `testing.md` for how to run it, and `code-quality.md` for the
    policy: what it measures and how a figure is reported
  - decisions and their reasons: ADRs
- **Names and links.** Guides use lower-case kebab-case file names and Title
  Case headings, link to each other with relative paths, and end with a
  "Related Guides" list. Headings are link targets: they may be added or
  reordered, but renaming one means updating every link to it.
- **Same-commit updates.** A change to behavior, commands, constants, or the
  memory map updates the guide that owns it in the same commit. A change of
  direction adds an ADR or supersedes one.
- **Work plan.** Closing a `TODO.md` item checks it off where it stands, with
  the implementation reference and the evidence that meets its acceptance
  criterion. A release gate that needs a physical board stays unchecked until
  hardware evidence exists, even when the supporting code is merged.

## Alternatives Considered

- **One long README.** This was the state from 2026-02-21 to 2026-09-26. It
  kept everything in one place, but the entry point became hard to scan, and
  there was nowhere to put a runbook, a security reference, or rationale
  without making it longer.
- **Keep the 2026-09-28 upper-case layout.** It already split the README, but
  it kept the mixed security and operations documents, lacked the feature
  catalog and data model, and differed from the sibling project's names.
- **A generated documentation site or a wiki.** A wiki is not versioned with
  the code, so documentation could not change in the same commit as behavior.
  A site generator such as mdBook adds a build step without changing what must
  be written. API documentation stays in rustdoc, which CI builds with
  warnings denied (`cargo doc --locked --no-deps --lib`).
- **Track work only in GitHub issues.** Issues suit reports and discussion, and
  the issue templates feed them, but acceptance criteria and closing evidence
  need to be versioned and reviewed with the code they describe.
- **A remaining-work-only roadmap.** The sibling project's `TODO.md` lists only
  open work and leaves finished work to features and history. The owner
  rejected that for bt2usb: a plan that drops finished tasks hides how far the
  work has come and where each piece was implemented.
- **Rationale only in commit messages and code comments.** That was the state
  before this decision. Commit messages are hard to find once later commits
  touch the same code, and comments explain local mechanics, not trade-offs
  between options.

## Rationale

Each reader gets one stable starting point:

- someone deciding whether to build one starts with features and hardware
- a person bringing up a board starts with first flash
- contributors start with development, code quality, and testing
- reviewers of design changes start with architecture, the data model, and the
  ADRs
- whoever ships a build starts with deployment
- whoever is debugging a unit starts with operations
- security reviewers start with security, and reporters with `SECURITY.md`
- anyone asking "what is left, and what is finished?" opens `TODO.md`

A complete `TODO.md` answers both progress questions in one list. Checked items
carry source references, so a reviewer can trace a "done" claim to code; open
items carry acceptance criteria, so nobody has to guess when one is finished.
`features.md` then stays a description of behavior for users rather than a
history of work.

Keeping `SECURITY.md` at the root preserves GitHub's security-policy
integration, and `.github/ISSUE_TEMPLATE/config.yml` points vulnerability
reporters there instead of to a public issue. The detailed reference lives with
the other guides, where it can link to the data model and deployment.

ADRs make the reasons reviewable next to the code they explain. Several of the
project's constraints are invisible in the code alone: why `memory_sd.x`
asserts the position of `__sdata`, why a corrupted store disables writes, why
release packaging never rebuilds. Each now has a record that says what was
decided, what it replaced, and what it costs.

## Consequences

Positive:

- The README stays short as the guides grow, and each reader has an obvious
  first page.
- Decisions have a durable, reviewable home, and the roadmap decisions that
  still need an ADR are listed in architecture.
- Progress is visible in one list, with done work traceable to source and open
  work measurable against its criterion.

Negative:

- There are more files to keep consistent. A behavior change can touch a
  guide, `TODO.md`, and an ADR at once.
- Cross-links and documented constants can go stale silently until automated
  checks exist.
- `features.md` and the checked items in `TODO.md` describe overlapping
  ground from different angles; both must change when a capability changes.

Follow-up obligations:

- Update the owning guide, `TODO.md`, and any affected ADR in the same commit
  as the change they describe.
- Never describe an open item as a feature, and never check off a hardware
  gate without board evidence.
- Close the "Automated documentation checks" item in [TODO.md](../../TODO.md)
  so a broken link or a stale constant fails CI.

## Implementation

- The documents in the table above, restructured in `7fc99d6`. Its commit
  renamed the guides, turned `RELEASING.md` into `deployment.md`, moved
  flashing and release gates out of the operations runbook, split the security
  reference from the reporting policy, and added `features.md`,
  `data-model.md`, the ADR log, and the issue templates.
- The ADR process, triggers, statuses, template, and index are in
  [architecture](../architecture.md#adr-process); the index of accepted records
  is under [accepted ADRs](../architecture.md#accepted-adrs).
- Code comments point at guide paths and must follow a rename: the binary
  comments in `Cargo.toml`, the module headers of `src/sim.rs`,
  `src/selftest.rs`, `src/ble/coordinator.rs` and `src/ui/ui_logic.rs`, three
  recipes in `maskfile.md`, and the closing message of
  `.devcontainer/post-create.sh`.
- Issue templates: `.github/ISSUE_TEMPLATE/bug.md`, `feature.md`, and
  `hardware-result.md` (labelled `hardware-evidence`, it asks for the commit,
  ELF hash, SoftDevice version, and every checklist result); `config.yml` links
  vulnerability reports to `SECURITY.md`.

### Verification Status

- **Implemented:** the documents in the table above, the issue templates, and
  the ADR log.
- **Software-verified:** by a manual, repository-wide check that every
  relative link between Markdown files, including its `#anchor`, resolves.
  The 2026-10-09 check of the restructured documents found no broken link.
  The [2026-09-28 validation record](../testing.md#validation-record--2026-09-28)
  also lists local links as validated for the earlier layout. No CI job checks
  links or documented constants, so nothing catches a link or constant that
  goes stale on a later commit.
- **Hardware-verified:** not applicable.

## Related

- [README](../../README.md), [TODO.md](../../TODO.md), and the
  [security policy](../../SECURITY.md)
- [Architecture overview and ADR index](../architecture.md)
- [Features](../features.md)
- [Development](../development.md) and [code quality](../code-quality.md)
- [Testing](../testing.md)
- [ADR 0004: Verify in layers](0004-layered-verification.md), which defines
  the verification levels every document must state
- [ADR 0013: Pinned toolchain and mask tasks](0013-pinned-toolchain-and-mask-tasks.md),
  which makes `maskfile.md` the executable task reference
