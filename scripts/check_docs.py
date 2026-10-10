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

A line ending in ``<!-- check-docs: ignore -->`` is skipped by the config,
memory, and commands checks, for a sentence that quotes an old value on
purpose. Validation records (headings that start with "Validation Record")
are skipped by the same checks, because they describe the tree as it was.
"""

from __future__ import annotations

import argparse
import ast
from dataclasses import dataclass, field
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib


CHECKS = ("links", "config", "memory", "commands")
IGNORE_MARKER = "<!-- check-docs: ignore -->"
HISTORY_HEADING = re.compile(r"^#{1,6}\s+Validation Record\b")
DEFAULTS_DOC = "docs/hardware.md"
DEFAULTS_HEADING = "Configuration Defaults"

# Units in which a constant's raw value is counted, for inline mentions that
# give the value in milliseconds or seconds. Names ending in _SECS or _MS are
# derived from the suffix.
CONST_UNITS_MS = {
    "BLE_SUP_TIMEOUT": 10.0,
    "BLE_MIN_SUP_TIMEOUT": 10.0,
    "BLE_CONN_INTERVAL_MIN": 1.25,
    "BLE_CONN_INTERVAL_MAX": 1.25,
    "BLE_CONN_EVENT_LENGTH": 1.25,
    "BLE_PEER_MAX_CONN_INTERVAL": 1.25,
    "BLE_FAST_SCAN_INTERVAL": 0.625,
    "BLE_FAST_SCAN_WINDOW": 0.625,
}
TIME_UNITS_MS = {
    "ms": 1.0, "millisecond": 1.0, "milliseconds": 1.0,
    "s": 1000.0, "second": 1000.0, "seconds": 1000.0,
}
NUMBER = r"0x[0-9A-Fa-f_]+|[0-9][0-9_,]*(?:\.[0-9]+)?"
UNIT = r"ms|milliseconds?|s|seconds?"
INLINE_AFTER = re.compile(
    rf"`(?P<name>[A-Z][A-Z0-9_]+)`\s+\((?P<value>{NUMBER})(?:\s*(?P<unit>{UNIT}))?\)"
)
INLINE_BEFORE = re.compile(
    rf"(?<![\w.-])(?P<value>{NUMBER})(?:[ -](?P<unit>{UNIT}))?\s+\(`(?P<name>[A-Z][A-Z0-9_]+)`\)"
)

ADDRESS_RANGE = re.compile(r"`(0x[0-9A-Fa-f_]+)\s*[–-]\s*(0x[0-9A-Fa-f_]+)`")
PAGE_RANGE = re.compile(r"\bpages\s+(\d+)\s*(?:–|-|to)\s*(\d+)\b")

# File names that documentation may mention although no tracked file has the
# name, each with the reason. Generated outputs are matched by pattern below.
KNOWN_ABSENT = {
    "docs/FIRST_FLASH.md": "renamed to docs/first-flash.md in 7fc99d6",
    "src/hid/held.rs": "removed in 2479c79; ADR 0005 records why",
    "held.rs": "removed in 2479c79; ADR 0005 records why",
    "chips/nrf52840.rs": "a source file of embassy-nrf, not of this repository",
    "platforms/cpus/nrf52840.repl": "the platform description that ships with Renode",
}
# Outputs that builds and tools write; never tracked.
GENERATED = re.compile(r"^(OUT_DIR|target|dist|coverage|coverage-html|\.vscode)/")
PATH_SUFFIXES = (
    ".rs", ".toml", ".x", ".py", ".sh", ".yml", ".yaml", ".md",
    ".robot", ".resc", ".repl", ".cs", ".json", ".lock",
)
PROPOSED_ADR = re.compile(r"^- Status: Proposed\b", re.M)


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


# Links -----------------------------------------------------------------------

def slug(heading: str) -> str:
    """GitHub's anchor for a heading."""
    heading = re.sub(r"<[^>]+>", "", heading)
    heading = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", heading)
    heading = heading.replace("`", "").strip().lower()
    heading = re.sub(r"[^\w\- ]", "", heading)
    return heading.replace(" ", "-")


def anchors(doc: Document) -> set[str]:
    found: set[str] = set()
    seen: dict[str, int] = {}
    heading = re.compile(r"^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$")
    for blanked, source in zip(doc.prose.splitlines(), doc.text.splitlines()):
        # Fenced lines are blank in the prose copy; code spans in a heading
        # are blank there too, so the anchor text comes from the source line.
        if not heading.match(blanked):
            continue
        base = slug(heading.match(source).group(1))
        count = seen.get(base, 0)
        seen[base] = count + 1
        found.add(base if count == 0 else f"{base}-{count}")
    found.update(re.findall(r"<a\s+(?:id|name)=\"([^\"]+)\"", doc.text))
    return found


def check_links(root: Path, docs: dict[str, Document]) -> list[Finding]:
    findings: list[Finding] = []
    cache: dict[str, set[str]] = {}
    for doc in docs.values():
        for match in re.finditer(r"\]\(<?([^)\s>]+)>?(?:\s+\"[^\"]*\")?\)", doc.prose):
            target = match.group(1)
            if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", target):
                continue
            line = doc.line_of(match.start())
            path, _, fragment = target.partition("#")
            if path:
                resolved = os.path.normpath(os.path.join(os.path.dirname(doc.path), path))
            else:
                resolved = doc.path
            resolved = resolved.replace(os.sep, "/")
            if resolved.startswith("../") or not (root / resolved).exists():
                findings.append(Finding(doc.path, line, "links",
                                        f"link target {target} does not exist"))
                continue
            if not fragment or not resolved.endswith(".md"):
                continue
            if resolved not in cache:
                target_doc = docs.get(resolved) or Document(
                    resolved, (root / resolved).read_text(encoding="utf-8"))
                cache[resolved] = anchors(target_doc)
            if fragment not in cache[resolved]:
                findings.append(Finding(doc.path, line, "links",
                                        f"{resolved} has no heading for #{fragment}"))
    return findings


# Config ----------------------------------------------------------------------

def evaluate(expression: str, known: dict[str, object]) -> object:
    """Evaluate a constant expression from config.rs."""
    expression = re.sub(r"\bas\s+[a-z0-9]+\b", "", expression)
    expression = re.sub(r"\btrue\b", "True", re.sub(r"\bfalse\b", "False", expression))
    tree = ast.parse(expression.strip(), mode="eval")

    def value(node: ast.AST) -> object:
        if isinstance(node, ast.Expression):
            return value(node.body)
        if isinstance(node, ast.Constant) and isinstance(node.value, (int, str, bool)):
            return node.value
        if isinstance(node, ast.Name) and node.id in known:
            return known[node.id]
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
            return -value(node.operand)
        if isinstance(node, ast.BinOp):
            left, right = value(node.left), value(node.right)
            operators = {
                ast.Add: lambda a, b: a + b,
                ast.Sub: lambda a, b: a - b,
                ast.Mult: lambda a, b: a * b,
                ast.Div: lambda a, b: a // b,
            }
            for kind, apply in operators.items():
                if isinstance(node.op, kind):
                    return apply(left, right)
        raise ValueError(f"unsupported expression in src/config.rs: {expression.strip()}")

    return value(tree)


def config_constants(root: Path) -> dict[str, tuple[object, int]]:
    """Each ``pub const`` in src/config.rs: (value, line)."""
    source = (root / "src/config.rs").read_text(encoding="utf-8")
    constants: dict[str, tuple[object, int]] = {}
    values: dict[str, object] = {}
    pattern = re.compile(r"pub const (\w+)\s*:\s*[^=]+?=\s*(.+?);", re.S)
    for match in pattern.finditer(source):
        name, expression = match.groups()
        result = evaluate(expression, values)
        values[name] = result
        constants[name] = (result, source.count("\n", 0, match.start()) + 1)
    return constants


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


def expand_names(cell: str) -> list[str]:
    """`A_B_C` / `D` names A_B_C and A_B_D; full names stay as written."""
    names = re.findall(r"`([A-Za-z0-9_]+)`", cell)
    expanded: list[str] = []
    for name in names:
        if expanded and "_" not in name:
            name = expanded[0].rsplit("_", 1)[0] + "_" + name
        expanded.append(name)
    return expanded


def parse_number(text: str) -> float | None:
    text = text.replace("_", "").replace(",", "")
    try:
        return float(int(text, 16)) if text.lower().startswith("0x") else float(text)
    except ValueError:
        return None


def same_value(documented: str, actual: object) -> bool:
    documented = documented.strip().strip("`")
    if isinstance(actual, bool):
        return documented == ("true" if actual else "false")
    if isinstance(actual, str):
        return documented == actual
    number = parse_number(documented)
    return number is not None and number == float(actual)


def unit_ms(name: str) -> float | None:
    if name in CONST_UNITS_MS:
        return CONST_UNITS_MS[name]
    if name.endswith("_SECS"):
        return 1000.0
    if name.endswith("_MS"):
        return 1.0
    return None


def inline_matches(value: str, unit: str | None, name: str, actual: object) -> bool | None:
    """Whether an inline mention matches; None when it cannot be compared."""
    number = parse_number(value)
    if number is None or isinstance(actual, (bool, str)):
        return None
    if unit is None:
        return number == float(actual)
    factor = unit_ms(name)
    if factor is None:
        return None
    return abs(number * TIME_UNITS_MS[unit] - float(actual) * factor) < 1e-6


def check_config(root: Path, docs: dict[str, Document]) -> list[Finding]:
    findings: list[Finding] = []
    constants = config_constants(root)
    defaults = docs.get(DEFAULTS_DOC)
    if defaults is None:
        return [Finding(DEFAULTS_DOC, 1, "config", "file is missing")]
    listed: set[str] = set()
    rows = table_rows(defaults, DEFAULTS_HEADING)
    if not rows:
        findings.append(Finding(DEFAULTS_DOC, heading_line(defaults, DEFAULTS_HEADING), "config",
                                f"no table under the {DEFAULTS_HEADING!r} heading"))
    for line, cells in rows:
        names = expand_names(cells[0])
        values = [part.strip() for part in cells[1].split(" / ")] if len(cells) > 1 else []
        listed.update(names)
        if len(values) != len(names):
            findings.append(Finding(DEFAULTS_DOC, line, "config",
                                    f"{len(names)} names but {len(values)} values"))
            continue
        for name, documented in zip(names, values):
            if name not in constants:
                findings.append(Finding(DEFAULTS_DOC, line, "config",
                                        f"{name} is not a constant in src/config.rs"))
            elif not same_value(documented, constants[name][0]):
                findings.append(Finding(
                    DEFAULTS_DOC, line, "config",
                    f"{name} is documented as {documented} but src/config.rs:"
                    f"{constants[name][1]} sets {format_value(constants[name][0])}"))
    heading = heading_line(defaults, DEFAULTS_HEADING)
    for name, (value, line) in constants.items():
        if name not in listed:
            findings.append(Finding(
                DEFAULTS_DOC, heading, "config",
                f"src/config.rs:{line} defines {name} = {format_value(value)}, but the "
                f"{DEFAULTS_HEADING} table does not list it"))
    for doc in docs.values():
        for pattern in (INLINE_AFTER, INLINE_BEFORE):
            for match in pattern.finditer(doc.text):
                name = match.group("name")
                line = doc.line_of(match.start())
                if name not in constants or doc.skipped(line):
                    continue
                actual, source_line = constants[name]
                verdict = inline_matches(match.group("value"), match.group("unit"), name, actual)
                if verdict is False:
                    findings.append(Finding(
                        doc.path, line, "config",
                        f"{match.group(0)!r} disagrees with src/config.rs:{source_line}, "
                        f"which sets {name} = {format_value(actual)}"))
    return findings


def format_value(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, str):
        return f'"{value}"'
    return str(value)


# Memory ----------------------------------------------------------------------

def linker_regions(path: Path) -> dict[str, tuple[int, int]]:
    """MEMORY regions of a linker script: name -> (start, end exclusive)."""
    text = re.sub(r"/\*.*?\*/", "", path.read_text(encoding="utf-8"), flags=re.S)
    regions: dict[str, tuple[int, int]] = {}
    pattern = re.compile(
        r"(\w+)\s*:\s*ORIGIN\s*=\s*(0x[0-9A-Fa-f]+|\d+)\s*,\s*LENGTH\s*=\s*(\d+)\s*([KM]?)")
    for name, origin, length, scale in pattern.findall(text):
        start = int(origin, 0)
        size = int(length) * {"": 1, "K": 1024, "M": 1024 * 1024}[scale]
        regions[name] = (start, start + size)
    return regions


def memory_layout(root: Path) -> dict[str, tuple[int, int]]:
    """Documented region labels and the range each must show."""
    sd = linker_regions(root / "memory_sd.x")
    sim = linker_regions(root / "memory_sim.x")
    constants = {name: value for name, (value, _) in config_constants(root).items()}
    storage = (int(constants["STORAGE_FLASH_START"]), int(constants["STORAGE_FLASH_END"]))
    flash_end = sim["FLASH"][1]
    ram_base = sim["RAM"][0]
    return {
        "SoftDevice flash": (sim["FLASH"][0], sd["FLASH"][0]),
        "Application flash": sd["FLASH"],
        "`FLASH`": sd["FLASH"],
        "Pairing/bond storage": storage,
        "Pairing store": storage,
        "Unused tail flash": (storage[1], flash_end),
        "SoftDevice RAM reservation": (ram_base, sd["RAM"][0]),
        "Application RAM": sd["RAM"],
        "`RAM`": sd["RAM"],
        "Flash ([memory_sim.x](../memory_sim.x))": sim["FLASH"],
        "RAM": sim["RAM"],
    }


def check_memory(root: Path, docs: dict[str, Document]) -> list[Finding]:
    findings: list[Finding] = []
    layout = memory_layout(root)
    constants = {name: value for name, (value, _) in config_constants(root).items()}
    first_page = int(constants["STORAGE_FLASH_PAGE_START"])
    last_page = first_page + int(constants["STORAGE_FLASH_PAGE_COUNT"]) - 1
    page_size = int(constants["FLASH_PAGE_SIZE"])
    # A range that starts at one of these addresses must end where that region
    # ends. Address 0 and the RAM base start more than one region, so they are
    # checked only in tables.
    starts: dict[int, tuple[str, int]] = {}
    for label, (start, end) in layout.items():
        if start not in (0, layout["RAM"][0]):
            starts.setdefault(start, (label, end))
    for doc in docs.values():
        lines = doc.text.splitlines()
        for number, line in enumerate(lines, start=1):
            if doc.skipped(number):
                continue
            if line.startswith("|"):
                cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
                if cells and cells[0] in layout and len(cells) >= 3:
                    findings += table_region(doc.path, number, cells, layout[cells[0]])
            for match in ADDRESS_RANGE.finditer(line):
                start, end = (int(value.replace("_", ""), 16) for value in match.groups())
                if start in starts and end != starts[start][1]:
                    label, expected = starts[start]
                    findings.append(Finding(
                        doc.path, number, "memory",
                        f"{match.group(0)} starts {label} but ends at {end:#x}; "
                        f"the region ends at {expected:#x}"))
            for match in PAGE_RANGE.finditer(line):
                low, high = int(match.group(1)), int(match.group(2))
                context = line[max(0, match.start() - 80):match.end() + 80].lower()
                if not re.search(r"pairing|bond|storage|0xf[0-9a-f]{4}|0x000f", context):
                    continue
                if (low, high) != (first_page, last_page):
                    findings.append(Finding(
                        doc.path, number, "memory",
                        f"pages {low}–{high} disagree with STORAGE_FLASH_PAGE_START/COUNT, "
                        f"which reserve pages {first_page}–{last_page} "
                        f"({first_page * page_size:#x}–{(last_page + 1) * page_size:#x})"))
    return findings


def table_region(path: str, line: int, cells: list[str],
                 expected: tuple[int, int]) -> list[Finding]:
    match = ADDRESS_RANGE.search(cells[1])
    if not match:
        return []  # a row about the region that gives no range, e.g. a limit
    start, end = (int(value.replace("_", ""), 16) for value in match.groups())
    found: list[Finding] = []
    if (start, end) != expected:
        found.append(Finding(
            path, line, "memory",
            f"{cells[0]} is documented as {start:#010x}–{end:#010x} but the linker "
            f"scripts and src/config.rs give {expected[0]:#010x}–{expected[1]:#010x}"))
    size = re.match(r"([0-9]+)\s*KiB", cells[2])
    if size and int(size.group(1)) * 1024 != expected[1] - expected[0]:
        found.append(Finding(
            path, line, "memory",
            f"{cells[0]} is documented as {size.group(1)} KiB but spans "
            f"{(expected[1] - expected[0]) // 1024} KiB"))
    return found


# Commands --------------------------------------------------------------------

def mask_recipes(root: Path) -> set[str]:
    text = (root / "maskfile.md").read_text(encoding="utf-8")
    return set(re.findall(r"^## ([a-z0-9-]+)\s*$", text, re.M))


def cargo_targets(root: Path) -> tuple[set[str], set[str]]:
    with (root / "Cargo.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    bins = {target["name"] for target in manifest.get("bin", [])}
    features = set(manifest.get("features", {}))
    return bins, features


def tracked_files(root: Path) -> list[str]:
    try:
        output = subprocess.run(
            ["git", "ls-files", "-co", "--exclude-standard"], cwd=root,
            capture_output=True, text=True, check=True).stdout
        return [line for line in output.splitlines() if line]
    except (OSError, subprocess.CalledProcessError):
        files = []
        for directory, subdirectories, names in os.walk(root):
            subdirectories[:] = [d for d in subdirectories if d not in (".git", "target")]
            for name in names:
                files.append(os.path.relpath(os.path.join(directory, name), root)
                             .replace(os.sep, "/"))
        return files


def looks_like_path(text: str) -> bool:
    """A repository path (with a directory) or a Rust file name.

    Bare names of other kinds, such as ``memory.x`` or ``rustfmt.toml``, are
    as often generated or deliberately absent files as tracked ones.
    """
    if not re.fullmatch(r"[\w.-]*\w[\w./-]*", text) or text.startswith(("-", "/")):
        return False
    if not text.endswith(PATH_SUFFIXES) or not Path(text).suffix:
        return False
    return "/" in text or text.endswith(".rs")


def check_commands(root: Path, docs: dict[str, Document]) -> list[Finding]:
    findings: list[Finding] = []
    recipes = mask_recipes(root)
    bins, features = cargo_targets(root)
    files = tracked_files(root)
    names = {Path(name).name for name in files}
    for doc in docs.values():
        snippets = [(doc.line_of(offset), text) for offset, text in doc.code_spans]
        snippets += doc.code_lines
        for line, text in snippets:
            if doc.skipped(line):
                continue
            for match in re.finditer(r"(?:^|[;&|(]\s*|\$\s*)mask\s+([a-z][a-z0-9-]*)", text):
                if match.group(1) not in recipes:
                    findings.append(Finding(doc.path, line, "commands",
                                            f"mask has no recipe named {match.group(1)!r}"))
            if "cargo" in text or text.lstrip().startswith("--"):
                for match in re.finditer(r"--bin[ =]([\w-]+)", text):
                    if match.group(1) not in bins:
                        findings.append(Finding(
                            doc.path, line, "commands",
                            f"Cargo.toml has no binary named {match.group(1)!r}"))
                for match in re.finditer(r"--features[ =]([\w,-]+)", text):
                    for feature in match.group(1).split(","):
                        if feature not in features:
                            findings.append(Finding(
                                doc.path, line, "commands",
                                f"Cargo.toml has no feature named {feature!r}"))
        if PROPOSED_ADR.search(doc.text):
            continue  # a proposed decision names files it would add
        for line, text in [(doc.line_of(offset), text) for offset, text in doc.code_spans]:
            candidate = text.strip().removeprefix("./")
            while candidate.startswith("../"):
                candidate = candidate[3:]
            if doc.skipped(line) or not looks_like_path(candidate):
                continue
            if GENERATED.search(candidate) or candidate in KNOWN_ABSENT:
                continue
            if "/" in candidate:
                exists = any(name == candidate or name.endswith("/" + candidate)
                             for name in files)
            else:
                exists = candidate in names
            if not exists:
                findings.append(Finding(doc.path, line, "commands",
                                        f"no file in the repository matches {candidate}"))
    return findings


# Driver ----------------------------------------------------------------------

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


def run(root: Path, only: tuple[str, ...] = CHECKS) -> list[Finding]:
    docs = load_documents(root)
    runners = {
        "links": check_links,
        "config": check_config,
        "memory": check_memory,
        "commands": check_commands,
    }
    findings: list[Finding] = []
    for name in only:
        findings += runners[name](root, docs)
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
        print(f"{len(findings)} documentation finding(s) in {docs} Markdown files "
              f"({', '.join(args.only)})", file=sys.stderr)
        return 1
    print(f"Documentation checks passed: {', '.join(args.only)} over {docs} Markdown files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
