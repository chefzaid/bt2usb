"""The testing guide's Test Map gives each file's `#[test]` count.

Counts are lines containing ``#[test]``, as ``grep -c '#\\[test\\]'`` counts
them. Every Test Map row whose first cell links a file must give that file's
count, every Rust file under ``src/`` or ``tests/`` with a test needs a row,
and the sentence "The tree holds N `#[test]` functions: M in files compiled
into the host library, K in `tests/x.rs`, ..." must add up.
"""

from __future__ import annotations

import re
from pathlib import Path

from .model import Document, Finding, heading_line, tracked_files

GUIDE = "docs/testing.md"
ROW = re.compile(r"^\| \[[^\]]+\]\(\.\./(?P<path>[^)#]+)\)[^|]*\| (?P<count>\d+) \|")
TOTAL = re.compile(
    r"holds (?P<total>\d+) `#\[test\]` functions:\s+(?P<lib>\d+) in\s+files compiled"
    r"\s+into the host library"
)
TARGET = re.compile(r"(?P<count>\d+) in\s+`(?P<path>tests/[\w.-]+\.rs)`")


def count_tests(path: Path) -> int:
    return sum("#[test]" in line for line in path.read_text(encoding="utf-8").splitlines())


def check(root: Path, docs: dict[str, Document]) -> list[Finding]:
    doc = docs.get(GUIDE)
    if doc is None:
        return []
    findings: list[Finding] = []
    counts: dict[str, int] = {}
    for name in tracked_files(root):
        if re.match(r"(src|tests)/.*\.rs$", name) and (root / name).is_file():
            count = count_tests(root / name)
            if count:
                counts[name] = count

    def report(line: int, message: str) -> None:
        if not doc.skipped(line):
            findings.append(Finding(GUIDE, line, "tests", message))

    listed: set[str] = set()
    for number, line in enumerate(doc.text.splitlines(), start=1):
        row = ROW.match(line)
        if not row or not (root / row.group("path")).is_file():
            continue
        path, said = row.group("path"), int(row.group("count"))
        listed.add(path)
        real = counts.get(path, 0)
        if real != said:
            report(number, f"{path} has {real} #[test] functions, not {said}")
    for path in sorted(set(counts) - listed):
        report(heading_line(doc, "Test Map"), f"{path} has tests but no Test Map row")

    total = TOTAL.search(doc.text)
    if total:
        line = doc.line_of(total.start())
        library = sum(count for name, count in counts.items() if name.startswith("src/"))
        for said, real, what in (
            (int(total.group("total")), sum(counts.values()), "the tree holds"),
            (int(total.group("lib")), library, "the host library holds"),
        ):
            if said != real:
                report(line, f"{what} {real} #[test] functions, not {said}")
        for target in TARGET.finditer(doc.text, total.end(), total.end() + 400):
            path, said = target.group("path"), int(target.group("count"))
            if counts.get(path, 0) != said:
                report(
                    doc.line_of(target.start()),
                    f"{path} has {counts.get(path, 0)} #[test] functions, not {said}",
                )
    return findings
