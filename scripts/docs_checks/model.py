"""Markdown documents, findings, and the helpers every check shares."""

from __future__ import annotations

import os
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

IGNORE_MARKER = "<!-- check-docs: ignore -->"
HISTORY_HEADING = re.compile(r"^#{1,6}\s+Validation Record\b")


@dataclass(frozen=True)
class Finding:
    path: str
    line: int
    check: str
    message: str

    def __str__(self) -> str:
        return f"{self.path}:{self.line}: [{self.check}] {self.message}"


@dataclass
class Document:
    """One Markdown file, with code blanked out of ``prose``."""

    path: str
    text: str
    prose: str = ""
    code_spans: list[tuple[int, str]] = field(default_factory=list)
    code_lines: list[tuple[int, str]] = field(default_factory=list)
    skipped_lines: set[int] = field(default_factory=set)

    def __post_init__(self) -> None:
        self.prose, self.code_spans, self.code_lines = split_code(self.text)
        self.skipped_lines = history_lines(self.text, self.prose)

    def line_of(self, offset: int) -> int:
        return self.text.count("\n", 0, offset) + 1

    def skipped(self, line: int) -> bool:
        return line in self.skipped_lines


def split_code(text: str) -> tuple[str, list[tuple[int, str]], list[tuple[int, str]]]:
    """Blank fenced blocks and code spans, keeping offsets and line breaks.

    Returns the blanked prose, the code spans as (offset, content), and the
    fenced lines as (line number, content).
    """
    out: list[str] = []
    spans: list[tuple[int, str]] = []
    fenced: list[tuple[int, str]] = []
    fence: str | None = None
    offset = 0
    for number, line in enumerate(text.splitlines(keepends=True), start=1):
        stripped = line.lstrip()
        marker = re.match(r"(`{3,}|~{3,})", stripped)
        if fence is None and marker:
            fence = marker.group(1)
            out.append(blank(line))
        elif fence is not None:
            if stripped.startswith(fence):
                fence = None
            else:
                fenced.append((number, line.rstrip("\n")))
            out.append(blank(line))
        else:

            def keep(match: re.Match[str], base: int = offset) -> str:
                spans.append((base + match.start(), match.group(2)))
                return blank(match.group(0))

            out.append(re.sub(r"(`+)(.+?)\1", keep, line))
        offset += len(line)
    return "".join(out), spans, fenced


def blank(text: str) -> str:
    return re.sub(r"[^\n]", " ", text)


def history_lines(text: str, prose: str) -> set[int]:
    """Lines inside validation records, or ending in the ignore marker.

    Headings are read from the prose copy, so a ``#`` comment in a fenced
    block does not open or close a record.
    """
    skipped: set[int] = set()
    level = 0
    lines = zip(text.splitlines(), prose.splitlines())
    for number, (line, blanked) in enumerate(lines, start=1):
        heading = re.match(r"^(#{1,6})\s", blanked)
        if heading:
            depth = len(heading.group(1))
            if HISTORY_HEADING.match(line):
                level = depth
            elif level and depth <= level:
                level = 0
        if level or line.rstrip().endswith(IGNORE_MARKER):
            skipped.add(number)
    return skipped


def table_rows(doc: Document, heading: str) -> list[tuple[int, list[str]]]:
    """Rows of the first table under ``heading``, as (line, raw cells)."""
    lines = doc.text.splitlines()
    rows: list[tuple[int, list[str]]] = []
    inside = False
    for number, line in enumerate(lines, start=1):
        if re.match(rf"^#{{1,6}}\s+{re.escape(heading)}\s*$", line):
            inside = True
            continue
        if not inside:
            continue
        if line.startswith("#"):
            break
        if line.startswith("|"):
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            if not all(re.fullmatch(r":?-+:?", cell) for cell in cells):
                rows.append((number, cells))
        elif rows:
            break
    return rows[1:]  # drop the header row


def heading_line(doc: Document, heading: str) -> int:
    """The line of ``heading`` in ``doc``, or 1 when it is missing."""
    for number, line in enumerate(doc.text.splitlines(), start=1):
        if re.match(rf"^#{{1,6}}\s+{re.escape(heading)}\s*$", line):
            return number
    return 1


def tracked_files(root: Path) -> list[str]:
    try:
        output = subprocess.run(
            ["git", "ls-files", "-co", "--exclude-standard"],
            cwd=root,
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        return [line for line in output.splitlines() if line]
    except (OSError, subprocess.CalledProcessError):
        files = []
        for directory, subdirectories, names in os.walk(root):
            subdirectories[:] = [d for d in subdirectories if d not in (".git", "target")]
            for name in names:
                files.append(
                    os.path.relpath(os.path.join(directory, name), root).replace(os.sep, "/")
                )
        return files


def load_documents(root: Path) -> dict[str, Document]:
    docs: dict[str, Document] = {}
    for name in tracked_files(root):
        if not name.endswith(".md"):
            continue
        if name.startswith("vendor/") and not name.endswith("README.bt2usb.md"):
            continue
        if not (root / name).is_file():
            continue
        docs[name] = Document(name, (root / name).read_text(encoding="utf-8"))
    return docs
