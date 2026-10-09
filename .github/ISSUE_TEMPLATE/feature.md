---
name: Feature or roadmap item
about: Propose new behavior or a TODO.md item
title: ""
labels: enhancement
assignees: ""
---

> Check the [work plan](https://github.com/chefzaid/bt2usb/blob/main/TODO.md)
> first; if the item is already there, reference it instead of restating it.

## Outcome

Describe what changes for the person using the bridge, not only the implementation.

## Acceptance criteria

- [ ] Behavior is objectively verifiable, in software and, where needed, on hardware.
- [ ] Failure paths are covered: link loss, USB reset/suspend, full queues, storage errors.
- [ ] Held input is always released.
- [ ] Memory, flash, and power impact are estimated.
- [ ] Security, storage-format migration, and rollback impact are assessed against
      the [threat model](https://github.com/chefzaid/bt2usb/blob/main/docs/security.md#threat-model).
- [ ] An ADR is needed under the
      [ADR process](https://github.com/chefzaid/bt2usb/blob/main/docs/architecture.md#adr-process): yes / no.

## Notes

List dependencies, hardware needed for validation, and follow-up work.
