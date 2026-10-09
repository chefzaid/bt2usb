# ADR 0001: Keep The README Short And Organize Detailed Docs By Reader

- Status: Accepted
- Date: 2026-10-09

## Context

bt2usb's documentation grew alongside the hardening work: user controls,
hardware wiring, task architecture, storage format, build commands, Renode,
release provenance, first-flash acceptance, recovery, and security limits. The
material was accurate but spread across an upper-case `docs/` set, a long root
README, and a root `SECURITY.md` that mixed reporting policy with the security
design. There was no durable place to record *why* the firmware is built the
way it is, so rationale lived only in commit messages and code comments.

The sibling project `swirl-demo-app` already uses a reader-oriented `docs/`
structure with an ADR log. Following the same shape lets the projects be read
the same way without copying web-application topics that do not apply here.

## Decision

Keep `README.md` as the short entry point: what the device is, a quick start,
the documentation map, the roadmap pointer, and the license. Move detailed
material into lower-case, kebab-case guides:

- `docs/features.md`: implemented capabilities, controls, and current boundaries
- `docs/architecture.md`: runtime architecture and the ADR index
- `docs/data-model.md`: persisted pairing store, HID report contracts, and memory ownership
- `docs/hardware.md`: parts, wiring, compile-time configuration, and memory map
- `docs/development.md`: toolchain, commands, and contribution workflow
- `docs/testing.md`: verification layers, CI, and validation records
- `docs/first-flash.md`: board bring-up and hardware acceptance checklist
- `docs/deployment.md`: releases, provenance verification, and flashing
- `docs/operations.md`: recovery and diagnostics runbook
- `docs/security.md`: trust boundaries, controls, and limitations
- `docs/adr/`: architecture decision records

Keep root `TODO.md` as the single backlog, root `maskfile.md` as the executable
task reference, and root `SECURITY.md` as the short vulnerability-reporting
policy that GitHub surfaces.

Documentation must distinguish implemented behavior from planned behavior, and
software-verified behavior from hardware-verified behavior.

## Rationale

Each reader gets a stable starting point:

- someone deciding whether to build one starts with features and hardware
- a person bringing up a board starts with first flash
- contributors start with development and testing
- reviewers of design changes start with architecture, the data model, and ADRs
- whoever ships a build starts with deployment
- whoever is debugging a unit starts with operations
- security reviewers start with security

Lower-case kebab-case names match the sibling projects and keep links
predictable. Keeping `SECURITY.md` at the root preserves GitHub's security
policy integration while the detailed reference lives with the other guides.

## Consequences

The README stays readable as the guides grow. Every guide ends with related
links, so cross-links become a maintenance responsibility: a change to
behavior, commands, constants, or the memory map updates the relevant guide,
and a change of direction adds or supersedes an ADR, in the same commit.

Roadmap items cannot be described as features because a dependency, constant,
or partial implementation exists. Hardware acceptance stays open until board
evidence exists.
