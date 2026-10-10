"""``src/config.rs`` constants against the defaults table and inline mentions."""

from __future__ import annotations

import ast
import re
from pathlib import Path

from .model import Document, Finding, heading_line, table_rows

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
    "ms": 1.0,
    "millisecond": 1.0,
    "milliseconds": 1.0,
    "s": 1000.0,
    "second": 1000.0,
    "seconds": 1000.0,
}
NUMBER = r"0x[0-9A-Fa-f_]+|[0-9][0-9_,]*(?:\.[0-9]+)?"
UNIT = r"ms|milliseconds?|s|seconds?"
INLINE_AFTER = re.compile(
    rf"`(?P<name>[A-Z][A-Z0-9_]+)`\s+\((?P<value>{NUMBER})(?:\s*(?P<unit>{UNIT}))?\)"
)
INLINE_BEFORE = re.compile(
    rf"(?<![\w.-])(?P<value>{NUMBER})(?:[ -](?P<unit>{UNIT}))?\s+\(`(?P<name>[A-Z][A-Z0-9_]+)`\)"
)


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
    pattern = re.compile(r"pub const (\w+)\s*:\s*[^=]+?=\s*(.+?);", re.DOTALL)
    for match in pattern.finditer(source):
        name, expression = match.groups()
        result = evaluate(expression, values)
        values[name] = result
        constants[name] = (result, source.count("\n", 0, match.start()) + 1)
    return constants


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


def check(root: Path, docs: dict[str, Document]) -> list[Finding]:
    findings: list[Finding] = []
    constants = config_constants(root)
    defaults = docs.get(DEFAULTS_DOC)
    if defaults is None:
        return [Finding(DEFAULTS_DOC, 1, "config", "file is missing")]
    listed: set[str] = set()
    rows = table_rows(defaults, DEFAULTS_HEADING)
    if not rows:
        findings.append(
            Finding(
                DEFAULTS_DOC,
                heading_line(defaults, DEFAULTS_HEADING),
                "config",
                f"no table under the {DEFAULTS_HEADING!r} heading",
            )
        )
    for line, cells in rows:
        names = expand_names(cells[0])
        values = [part.strip() for part in cells[1].split(" / ")] if len(cells) > 1 else []
        listed.update(names)
        if len(values) != len(names):
            findings.append(
                Finding(
                    DEFAULTS_DOC, line, "config", f"{len(names)} names but {len(values)} values"
                )
            )
            continue
        for name, documented in zip(names, values):
            if name not in constants:
                findings.append(
                    Finding(
                        DEFAULTS_DOC, line, "config", f"{name} is not a constant in src/config.rs"
                    )
                )
            elif not same_value(documented, constants[name][0]):
                findings.append(
                    Finding(
                        DEFAULTS_DOC,
                        line,
                        "config",
                        f"{name} is documented as {documented} but src/config.rs:"
                        f"{constants[name][1]} sets {format_value(constants[name][0])}",
                    )
                )
    heading = heading_line(defaults, DEFAULTS_HEADING)
    for name, (value, line) in constants.items():
        if name not in listed:
            findings.append(
                Finding(
                    DEFAULTS_DOC,
                    heading,
                    "config",
                    f"src/config.rs:{line} defines {name} = {format_value(value)}, but the "
                    f"{DEFAULTS_HEADING} table does not list it",
                )
            )
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
                    findings.append(
                        Finding(
                            doc.path,
                            line,
                            "config",
                            f"{match.group(0)!r} disagrees with src/config.rs:{source_line}, "
                            f"which sets {name} = {format_value(actual)}",
                        )
                    )
    return findings


def format_value(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, str):
        return f'"{value}"'
    return str(value)
