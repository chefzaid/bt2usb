"""``mask`` recipes, Cargo binaries and features, and file paths in code spans."""

from __future__ import annotations

import re
import tomllib
from pathlib import Path

from .model import Document, Finding, tracked_files

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
    ".rs",
    ".toml",
    ".x",
    ".py",
    ".sh",
    ".yml",
    ".yaml",
    ".md",
    ".robot",
    ".resc",
    ".repl",
    ".cs",
    ".json",
    ".lock",
)
PROPOSED_ADR = re.compile(r"^- Status: Proposed\b", re.MULTILINE)


def mask_recipes(root: Path) -> set[str]:
    text = (root / "maskfile.md").read_text(encoding="utf-8")
    return set(re.findall(r"^## ([a-z0-9-]+)\s*$", text, re.MULTILINE))


def cargo_targets(root: Path) -> tuple[set[str], set[str]]:
    with (root / "Cargo.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    bins = {target["name"] for target in manifest.get("bin", [])}
    features = set(manifest.get("features", {}))
    return bins, features


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


def check(root: Path, docs: dict[str, Document]) -> list[Finding]:
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
                    findings.append(
                        Finding(
                            doc.path,
                            line,
                            "commands",
                            f"mask has no recipe named {match.group(1)!r}",
                        )
                    )
            if "cargo" in text or text.lstrip().startswith("--"):
                for match in re.finditer(r"--bin[ =]([\w-]+)", text):
                    if match.group(1) not in bins:
                        findings.append(
                            Finding(
                                doc.path,
                                line,
                                "commands",
                                f"Cargo.toml has no binary named {match.group(1)!r}",
                            )
                        )
                for match in re.finditer(r"--features[ =]([\w,-]+)", text):
                    for feature in match.group(1).split(","):
                        if feature not in features:
                            findings.append(
                                Finding(
                                    doc.path,
                                    line,
                                    "commands",
                                    f"Cargo.toml has no feature named {feature!r}",
                                )
                            )
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
                exists = any(name == candidate or name.endswith("/" + candidate) for name in files)
            else:
                exists = candidate in names
            if not exists:
                findings.append(
                    Finding(
                        doc.path, line, "commands", f"no file in the repository matches {candidate}"
                    )
                )
    return findings
