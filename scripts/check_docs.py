#!/usr/bin/env python3
"""Check the Markdown documentation against the repository it describes.

Python 3.11+; standard library only. Each finding names the file and line,
what the documentation says, and what the repository says instead.

Checks (all run by default; ``--only`` selects some):

links     Relative links resolve to a file, and ``#fragment`` to a heading or
          an explicit anchor in the target Markdown file.
config    Every ``pub const`` in ``src/config.rs`` is listed in the
          Configuration Defaults table of ``docs/hardware.md`` with its value,
          and inline mentions such as ``BLE_SCAN_DURATION_SECS`` (8 s) or
          4 seconds (``BLE_SUP_TIMEOUT``) match the constant.
memory    The memory-map tables, address ranges, and pairing page numbers
          match ``memory_sd.x``, ``memory_sim.x``, and the storage constants.
commands  ``mask`` recipes, ``--bin`` and ``--features`` names, and file
          paths written in code spans exist in the repository.
tests     Each Test Map row in ``docs/testing.md`` gives its file's
          ``#[test]`` count, every Rust file with tests has a row, and the
          totals sentence adds up.
status    The Status At A Glance table in ``TODO.md`` gives each section's
          done, open, and open P0 item counts, and their totals.

A line ending in ``<!-- check-docs: ignore -->`` is skipped by the config,
memory, commands, tests, and status checks, for a sentence that quotes an old
value on purpose. Validation records (headings that start with "Validation Record")
are skipped by the same checks, because they describe the tree as it was.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from docs_checks import commands, config, counts, links, memory, status
from docs_checks.model import Finding, load_documents

CHECKS = ("links", "config", "memory", "commands", "tests", "status")
RUNNERS = {
    "links": links.check,
    "config": config.check,
    "memory": memory.check,
    "commands": commands.check,
    "tests": counts.check,
    "status": status.check,
}


def run(root: Path, only: tuple[str, ...] = CHECKS) -> list[Finding]:
    docs = load_documents(root)
    findings: list[Finding] = []
    for name in only:
        findings += RUNNERS[name](root, docs)
    return sorted(set(findings), key=lambda f: (f.path, f.line, f.check, f.message))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--only", nargs="+", choices=CHECKS, default=list(CHECKS))
    args = parser.parse_args(argv)
    findings = run(args.root, tuple(args.only))
    for finding in findings:
        print(finding)
    docs = len(load_documents(args.root))
    if findings:
        print(
            f"{len(findings)} documentation finding(s) in {docs} Markdown files "
            f"({', '.join(args.only)})",
            file=sys.stderr,
        )
        return 1
    print(f"Documentation checks passed: {', '.join(args.only)} over {docs} Markdown files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
