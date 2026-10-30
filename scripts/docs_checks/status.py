"""TODO.md's Status At A Glance table counts each section's items.

An item is a top-level ``- [x]`` (done) or ``- [ ]`` (open) line; an open item
whose priority is ``**P0**`` also counts as open P0. Each row whose first cell
links a ``## `` section must give that section's three counts, every section
with items needs a row, and the Total row must give the sums over every
section.
"""

from __future__ import annotations

import re
from pathlib import Path

from .model import Document, Finding, heading_line, table_rows

PLAN = "TODO.md"
TABLE = "Status At A Glance"
ITEM = re.compile(r"^- \[(?P<state>[ x])\] (?P<p0>\*\*P0\*\*)?")
ROW = re.compile(r"\[(?P<section>[^\]]+)\]\(#[^)]+\)")
COLUMNS = ("done", "open", "open P0")


def count_items(doc: Document) -> dict[str, list[int]]:
    """Done, open, and open P0 items under each ``## `` heading, in order."""
    sections: dict[str, list[int]] = {}
    current = None
    for line in doc.prose.splitlines():
        heading = re.match(r"^## (.+?)\s*$", line)
        if heading:
            current = sections.setdefault(heading.group(1), [0, 0, 0])
            continue
        item = ITEM.match(line)
        if item and current is not None:
            done = item.group("state") == "x"
            current[0 if done else 1] += 1
            current[2] += not done and bool(item.group("p0"))
    return {name: counts for name, counts in sections.items() if any(counts)}


def check(root: Path, docs: dict[str, Document]) -> list[Finding]:
    doc = docs.get(PLAN)
    if doc is None:
        return []
    findings: list[Finding] = []
    sections = count_items(doc)
    listed: set[str] = set()
    for line, cells in table_rows(doc, TABLE):
        if doc.skipped(line) or len(cells) < 4:
            continue
        said = [int(re.sub(r"\D", "", cell) or 0) for cell in cells[1:4]]
        row = ROW.search(cells[0])
        if row:
            name = row.group("section")
            listed.add(name)
            real = sections.get(name, [0, 0, 0])
        elif "Total" in cells[0]:
            name = "the plan"
            real = [sum(counts[i] for counts in sections.values()) for i in range(3)]
        else:
            continue
        for column, s, r in zip(COLUMNS, said, real):
            if s != r:
                findings.append(
                    Finding(PLAN, line, "status", f"{name} has {r} {column} items, not {s}")
                )
    for name in sections:
        if name not in listed:
            findings.append(
                Finding(PLAN, heading_line(doc, TABLE), "status", f"{name} has items but no row")
            )
    return findings
