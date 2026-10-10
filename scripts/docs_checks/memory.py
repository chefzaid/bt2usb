"""Memory-map tables, address ranges, and pairing pages against the linker scripts."""

from __future__ import annotations

import re
from pathlib import Path

from .config import config_constants
from .model import Document, Finding

ADDRESS_RANGE = re.compile(r"`(0x[0-9A-Fa-f_]+)\s*[–-]\s*(0x[0-9A-Fa-f_]+)`")
PAGE_RANGE = re.compile(r"\bpages\s+(\d+)\s*(?:–|-|to)\s*(\d+)\b")


def linker_regions(path: Path) -> dict[str, tuple[int, int]]:
    """MEMORY regions of a linker script: name -> (start, end exclusive)."""
    text = re.sub(r"/\*.*?\*/", "", path.read_text(encoding="utf-8"), flags=re.DOTALL)
    regions: dict[str, tuple[int, int]] = {}
    pattern = re.compile(
        r"(\w+)\s*:\s*ORIGIN\s*=\s*(0x[0-9A-Fa-f]+|\d+)\s*,\s*LENGTH\s*=\s*(\d+)\s*([KM]?)"
    )
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


def check(root: Path, docs: dict[str, Document]) -> list[Finding]:
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
                    findings.append(
                        Finding(
                            doc.path,
                            number,
                            "memory",
                            f"{match.group(0)} starts {label} but ends at {end:#x}; "
                            f"the region ends at {expected:#x}",
                        )
                    )
            for match in PAGE_RANGE.finditer(line):
                low, high = int(match.group(1)), int(match.group(2))
                context = line[max(0, match.start() - 80) : match.end() + 80].lower()
                if not re.search(r"pairing|bond|storage|0xf[0-9a-f]{4}|0x000f", context):
                    continue
                if (low, high) != (first_page, last_page):
                    findings.append(
                        Finding(
                            doc.path,
                            number,
                            "memory",
                            f"pages {low}–{high} disagree with STORAGE_FLASH_PAGE_START/COUNT, "
                            f"which reserve pages {first_page}–{last_page} "
                            f"({first_page * page_size:#x}–{(last_page + 1) * page_size:#x})",
                        )
                    )
    return findings


def table_region(
    path: str, line: int, cells: list[str], expected: tuple[int, int]
) -> list[Finding]:
    match = ADDRESS_RANGE.search(cells[1])
    if not match:
        return []  # a row about the region that gives no range, e.g. a limit
    start, end = (int(value.replace("_", ""), 16) for value in match.groups())
    found: list[Finding] = []
    if (start, end) != expected:
        found.append(
            Finding(
                path,
                line,
                "memory",
                f"{cells[0]} is documented as {start:#010x}–{end:#010x} but the linker "
                f"scripts and src/config.rs give {expected[0]:#010x}–{expected[1]:#010x}",
            )
        )
    size = re.match(r"([0-9]+)\s*KiB", cells[2])
    if size and int(size.group(1)) * 1024 != expected[1] - expected[0]:
        found.append(
            Finding(
                path,
                line,
                "memory",
                f"{cells[0]} is documented as {size.group(1)} KiB but spans "
                f"{(expected[1] - expected[0]) // 1024} KiB",
            )
        )
    return found
