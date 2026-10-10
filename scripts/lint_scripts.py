#!/usr/bin/env python3
"""Lint the Python helpers, the shell scripts, and the maskfile.md recipes.

Runs Ruff over every tracked Python file (lint and format check, settings in
ruff.toml) and ShellCheck over every tracked ``*.sh`` file and every sh or bash
recipe in maskfile.md. CI pins both tools; ``mask lint-scripts`` runs the same
checks locally (docs/code-quality.md#python-and-shell-checks).

Each maskfile recipe is checked as its own script, padded with blank lines so
that a finding's line number is its line in maskfile.md. Mask passes a recipe's
options and arguments as environment variables, so the padded script assigns
them on its first line, where ShellCheck would otherwise report them as
referenced but never assigned.

Exit status: 0 when every check passes, 1 when any reports a finding, 2 when a
tool is missing.
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SHELLS = {"bash": "bash", "sh": "sh"}
HEADING = re.compile(r"^(#{2,6}) +([A-Za-z0-9_-]+)((?: +[(\[][A-Za-z0-9_-]+[)\]])*) *$")
ARGUMENT = re.compile(r"[(\[]([A-Za-z0-9_-]+)[)\]]")
OPTION = re.compile(r"^\* +([A-Za-z0-9_-]+) *$")
FENCE = re.compile(r"^(```|~~~)\s*([A-Za-z0-9_-]*)\s*$")


@dataclass
class Recipe:
    """One recipe's script and the variables mask sets for it."""

    name: str
    language: str
    first_line: int
    lines: list[str]
    variables: list[str]


def env_name(name: str) -> str:
    """Mask exposes option and argument names with dashes as underscores."""
    return name.replace("-", "_")


def recipes(text: str) -> list[Recipe]:
    """The first fenced script under each maskfile heading, in file order."""
    found: list[Recipe] = []
    name, variables, taken, options = "", [], True, False
    lines = text.splitlines()
    index = 0
    while index < len(lines):
        line = lines[index]
        heading = HEADING.match(line)
        option = OPTION.match(line)
        fence = FENCE.match(line)
        if line.strip() == "**OPTIONS**":
            options = True
        elif line and not line.startswith(("* ", " ", "\t")):
            options = False
        if heading:
            name = heading.group(2)
            variables = [env_name(arg) for arg in ARGUMENT.findall(heading.group(3))]
            taken = False
        elif option and options and name:
            variables.append(env_name(option.group(1)))
        elif fence:
            marker, language = fence.groups()
            end = index + 1
            while end < len(lines) and not lines[end].startswith(marker):
                end += 1
            if name and not taken:
                found.append(Recipe(name, language, index + 2, lines[index + 1 : end], variables))
                taken = True
            index = end
        index += 1
    return found


def padded(recipe: Recipe) -> str:
    """The recipe as a script whose line numbers match maskfile.md."""
    header = " ".join(f'"${{{variable}=}}"' for variable in recipe.variables)
    first = f": {header}" if header else ""
    gap = [""] * (recipe.first_line - 2)
    return "\n".join([first, *gap, *recipe.lines]) + "\n"


def tracked(root: Path, pattern: str) -> list[str]:
    """Tracked files matching a git pathspec, where ``*`` also crosses ``/``."""
    listed = subprocess.run(
        ["git", "ls-files", "--", pattern],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.split()
    return sorted(listed)


def run(command: list[str], root: Path, stdin: str | None = None) -> tuple[int, str]:
    """Run a linter and return its exit status and combined output."""
    result = subprocess.run(
        command, cwd=root, input=stdin, capture_output=True, text=True, check=False
    )
    return result.returncode, (result.stdout + result.stderr).strip()


def shell_recipes(root: Path) -> list[Recipe]:
    """The maskfile recipes written for a shell ShellCheck understands."""
    text = (root / "maskfile.md").read_text(encoding="utf-8")
    return [recipe for recipe in recipes(text) if recipe.language in SHELLS]


def check_recipes(checked: list[Recipe], root: Path, shellcheck: str) -> list[str]:
    """ShellCheck findings for the given recipes, as maskfile.md lines."""
    findings: list[str] = []
    for recipe in checked:
        shell = SHELLS[recipe.language]
        command = [shellcheck, f"--shell={shell}", "--format=gcc", "-"]
        status, output = run(command, root, padded(recipe))
        if status:
            findings += [
                f"maskfile.md:{line[2:]} (recipe {recipe.name})"
                for line in output.splitlines()
                if line.startswith("-:")
            ] or [f"maskfile.md: ShellCheck failed on recipe {recipe.name}: {output}"]
    return findings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT, help="repository root")
    root = parser.parse_args(argv).root
    tools = {name: shutil.which(name) for name in ("ruff", "shellcheck")}
    missing = [name for name, path in tools.items() if path is None]
    if missing:
        print(
            f"Install {' and '.join(missing)} first "
            "(docs/development.md#toolchain lists the pinned versions).",
            file=sys.stderr,
        )
        return 2
    ruff, shellcheck = tools["ruff"], tools["shellcheck"]
    scripts = tracked(root, "*.sh")
    python = tracked(root, "*.py")
    steps = [
        ("Ruff lint", [ruff, "check", *python]),
        ("Ruff format", [ruff, "format", "--check", *python]),
        ("ShellCheck scripts", [shellcheck, *scripts]),
    ]
    failed: list[str] = []
    for label, command in steps:
        status, output = run(command, root)
        if status:
            failed.append(label)
            print(f"{label} failed:\n{output}")
    checked = shell_recipes(root)
    findings = check_recipes(checked, root, shellcheck)
    if findings:
        failed.append("ShellCheck maskfile.md recipes")
        print("ShellCheck maskfile.md recipes failed:", *findings, sep="\n")
    if failed:
        print(f"{len(failed)} script check(s) failed: {', '.join(failed)}", file=sys.stderr)
        return 1
    print(
        f"Script checks passed: Ruff over {len(python)} Python files, ShellCheck over "
        f"{len(scripts)} shell scripts and {len(checked)} maskfile.md recipes"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
