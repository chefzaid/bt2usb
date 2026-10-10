"""Relative links resolve to a file, and fragments to a heading or anchor."""

from __future__ import annotations

import os
import re
from pathlib import Path

from .model import Document, Finding


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


def check(root: Path, docs: dict[str, Document]) -> list[Finding]:
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
                findings.append(
                    Finding(doc.path, line, "links", f"link target {target} does not exist")
                )
                continue
            if not fragment or not resolved.endswith(".md"):
                continue
            if resolved not in cache:
                target_doc = docs.get(resolved) or Document(
                    resolved, (root / resolved).read_text(encoding="utf-8")
                )
                cache[resolved] = anchors(target_doc)
            if fragment not in cache[resolved]:
                findings.append(
                    Finding(doc.path, line, "links", f"{resolved} has no heading for #{fragment}")
                )
    return findings
